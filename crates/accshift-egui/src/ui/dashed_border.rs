//! `border-style: dashed` on a rounded box, as Blink paints it.
//!
//! `BoxBorderPainter::DrawDashedDottedBoxSideFromPath` strokes the OUTER
//! rounded rect (`SkPath::addRRect`, start index 0: the top edge after the
//! top-left corner, clockwise) with a pen `2 * 1.1` times the border width,
//! clipped to the border band, so the inner half of the pen is what shows.
//! The dash pattern comes from `StrokeData::SetupPaintDashPathEffect`: a dash
//! of 3 widths and a gap of 2 (2 and 1 from 3 px up), the gap then stretched
//! or shrunk by `SelectBestDashGap` so a whole number of dashes fits the
//! path length, truncated to an integer. That length is Skia's
//! `SkContourMeasure`, which measures each corner conic by chords within half
//! a pixel, a little under the true arc. `SkDashPath` skips the first dash of
//! a closed contour and appends it after the last one, joined to it.
//!
//! Everything here is in device pixels; the band is rasterized once per size
//! with tiny-skia (a Skia port, same analytic anti-aliasing) as a white mask
//! and tinted when drawn.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{
    pos2, vec2, ColorImage, Context, Id, Painter, Pos2, Rect, TextureHandle, TextureOptions,
};
use resvg::tiny_skia;

use crate::theme::Rgba;
use crate::ui::snap;

/// `SK_ScalarRoot2Over2`, the weight of a quarter-circle conic.
const W2: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// `kMaxTValue` of `SkContourMeasure`.
const MAX_T: i32 = 0x3FFF_FFFF;
/// `CHEAP_DIST_LIMIT` at a resolution scale of 1.
const TOLERANCE: f32 = 0.5;

#[derive(Clone, Copy)]
enum Seg {
    Line(Pos2, Pos2),
    /// Start, control, end; weight [`W2`].
    Conic(Pos2, Pos2, Pos2),
}

fn eval_conic(p0: Pos2, p1: Pos2, p2: Pos2, t: f32) -> Pos2 {
    let a = (1.0 - t) * (1.0 - t);
    let b = 2.0 * W2 * t * (1.0 - t);
    let c = t * t;
    let d = a + b + c;
    pos2(
        (a * p0.x + b * p1.x + c * p2.x) / d,
        (a * p0.y + b * p1.y + c * p2.y) / d,
    )
}

/// One `SkContourMeasure::Segment`: the distance at its end, the t there,
/// and which path segment it measures.
#[derive(Clone, Copy)]
struct Measured {
    distance: f32,
    t: f32,
    seg: usize,
}

/// `compute_conic_segs`.
#[allow(clippy::too_many_arguments)]
fn conic_segs(
    out: &mut Vec<Measured>,
    seg: usize,
    (p0, p1, p2): (Pos2, Pos2, Pos2),
    mut distance: f32,
    mint: i32,
    min_pt: Pos2,
    maxt: i32,
    max_pt: Pos2,
) -> f32 {
    let halft = (mint + maxt) >> 1;
    let half_pt = eval_conic(p0, p1, p2, halft as f32 / MAX_T as f32);
    let mid = pos2((min_pt.x + max_pt.x) * 0.5, (min_pt.y + max_pt.y) * 0.5);
    let curvy = (half_pt.x - mid.x).abs().max((half_pt.y - mid.y).abs()) > TOLERANCE;
    if ((maxt - mint) >> 10) != 0 && curvy {
        distance = conic_segs(
            out,
            seg,
            (p0, p1, p2),
            distance,
            mint,
            min_pt,
            halft,
            half_pt,
        );
        distance = conic_segs(
            out,
            seg,
            (p0, p1, p2),
            distance,
            halft,
            half_pt,
            maxt,
            max_pt,
        );
    } else {
        let d = min_pt.distance(max_pt);
        let prev = distance;
        distance += d;
        if distance > prev {
            out.push(Measured {
                distance,
                t: maxt as f32 / MAX_T as f32,
                seg,
            });
        }
    }
    distance
}

/// The outer rounded rect as `SkPath::addRRect(rrect, kCW, 0)` builds it.
fn rrect_segments(w: f32, h: f32, r: f32) -> Vec<Seg> {
    let p = pos2;
    vec![
        Seg::Line(p(r, 0.0), p(w - r, 0.0)),
        Seg::Conic(p(w - r, 0.0), p(w, 0.0), p(w, r)),
        Seg::Line(p(w, r), p(w, h - r)),
        Seg::Conic(p(w, h - r), p(w, h), p(w - r, h)),
        Seg::Line(p(w - r, h), p(r, h)),
        Seg::Conic(p(r, h), p(0.0, h), p(0.0, h - r)),
        Seg::Line(p(0.0, h - r), p(0.0, r)),
        Seg::Conic(p(0.0, r), p(0.0, 0.0), p(r, 0.0)),
    ]
}

/// `SkContourMeasure` over the segments.
fn measure(segs: &[Seg]) -> Vec<Measured> {
    let mut out = Vec::new();
    let mut distance = 0.0f32;
    for (i, seg) in segs.iter().enumerate() {
        match *seg {
            Seg::Line(a, b) => {
                let prev = distance;
                distance += a.distance(b);
                if distance > prev {
                    out.push(Measured {
                        distance,
                        t: 1.0,
                        seg: i,
                    });
                }
            }
            Seg::Conic(a, c, b) => {
                distance = conic_segs(&mut out, i, (a, c, b), distance, 0, a, MAX_T, b);
            }
        }
    }
    out
}

/// `SkContourMeasure::distanceToSegment`: the measured segment holding
/// `distance` and the t there, interpolated linearly along its chord.
fn locate(measured: &[Measured], distance: f32) -> (usize, f32) {
    let index = measured
        .iter()
        .position(|m| m.distance >= distance)
        .unwrap_or(measured.len() - 1);
    let seg = measured[index];
    let (mut start_t, mut start_d) = (0.0, 0.0);
    if index > 0 {
        let prev = measured[index - 1];
        start_d = prev.distance;
        if prev.seg == seg.seg {
            start_t = prev.t;
        }
    }
    let t = start_t + (seg.t - start_t) * (distance - start_d) / (seg.distance - start_d);
    (index, t)
}

fn point_at(seg: Seg, t: f32) -> Pos2 {
    match seg {
        Seg::Line(a, b) => a + (b - a) * t,
        Seg::Conic(a, c, b) => eval_conic(a, c, b, t),
    }
}

/// `getSegment(start, stop)` as a polyline (conic pieces sampled finely).
fn segment_points(segs: &[Seg], measured: &[Measured], start: f32, stop: f32, out: &mut Vec<Pos2>) {
    let (i0, t0) = locate(measured, start);
    let (i1, t1) = locate(measured, stop);
    let (s0, s1) = (measured[i0].seg, measured[i1].seg);
    let mut push_piece = |seg: Seg, a: f32, b: f32| {
        let steps = match seg {
            Seg::Line(..) => 1,
            Seg::Conic(..) => 12,
        };
        for k in 0..=steps {
            let p = point_at(seg, a + (b - a) * k as f32 / steps as f32);
            if out.last().is_none_or(|q: &Pos2| q.distance(p) > 1e-4) {
                out.push(p);
            }
        }
    };
    if s0 == s1 {
        push_piece(segs[s0], t0, t1);
        return;
    }
    push_piece(segs[s0], t0, 1.0);
    for seg in segs.iter().take(s1).skip(s0 + 1) {
        push_piece(*seg, 0.0, 1.0);
    }
    push_piece(segs[s1], 0.0, t1);
}

/// `StrokeData::SelectBestDashGap`.
fn select_best_dash_gap(length: f32, dash: f32, gap: f32) -> f32 {
    let min_n = ((length + gap) / (dash + gap)).floor();
    let max_n = min_n + 1.0;
    // A closed path: as many gaps as dashes.
    let min_gap = (length - min_n * dash) / min_n;
    let max_gap = (length - max_n * dash) / max_n;
    if max_gap <= 0.0 || (min_gap - gap).abs() < (max_gap - gap).abs() {
        min_gap
    } else {
        max_gap
    }
}

/// The dash polylines of the outer rrect `w` x `h` (device px), radius `r`,
/// border width `bw`; one solid closed outline when no dash fits.
fn dash_polylines(w: f32, h: f32, r: f32, bw: f32) -> Vec<Vec<Pos2>> {
    let segs = rrect_segments(w, h, r);
    let measured = measure(&segs);
    let Some(length) = measured.last().map(|m| m.distance) else {
        return Vec::new();
    };
    let (dash_ratio, gap_ratio) = if bw >= 3.0 { (2.0, 1.0) } else { (3.0, 2.0) };
    let dash = bw * dash_ratio;
    // `const int length`: the path length truncated.
    let int_length = length.trunc();
    if int_length <= dash * 2.0 {
        let mut all = Vec::new();
        segment_points(&segs, &measured, 0.0, length, &mut all);
        return vec![all];
    }
    let gap = select_best_dash_gap(int_length, dash, bw * gap_ratio);
    let intervals = [dash, gap];
    // `SkDashPath::InternalFilter`, in doubles as Skia does.
    let mut out: Vec<Vec<Pos2>> = Vec::new();
    let mut distance = 0.0f64;
    let mut index = 0usize;
    let mut dlen = f64::from(dash);
    let mut skip_first = true;
    let mut added = false;
    while distance < f64::from(length) {
        added = false;
        if index.is_multiple_of(2) && !skip_first {
            added = true;
            let start = distance as f32;
            let stop = ((distance + dlen) as f32).min(length);
            if stop > start {
                let mut line = Vec::new();
                segment_points(&segs, &measured, start, stop, &mut line);
                out.push(line);
            }
        }
        distance += dlen;
        skip_first = false;
        index = (index + 1) % 2;
        dlen = f64::from(intervals[index]);
    }
    let mut first = Vec::new();
    segment_points(&segs, &measured, 0.0, dash, &mut first);
    match out.last_mut() {
        // Joined to the dash that ran into the end of the contour.
        Some(last) if added => last.extend(first),
        _ => out.push(first),
    }
    out
}

fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32, pb: &mut tiny_skia::PathBuilder) {
    // Cubic quarter circles (Skia's own conversion of a conic is as close).
    const K: f32 = 0.552_284_8;
    let r = r.max(0.0).min(w * 0.5).min(h * 0.5);
    let k = r * K;
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
}

/// The dashed band of a `w` x `h` device-px box, white on transparent.
fn raster(w: u32, h: u32, r: f32, bw: f32) -> Option<ColorImage> {
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let (wf, hf) = (w as f32, h as f32);
    let mut band = tiny_skia::PathBuilder::new();
    rrect_path(0.0, 0.0, wf, hf, r, &mut band);
    rrect_path(
        bw,
        bw,
        wf - 2.0 * bw,
        hf - 2.0 * bw,
        (r - bw).max(0.0),
        &mut band,
    );
    let band = band.finish()?;
    let mut mask = tiny_skia::Mask::new(w, h)?;
    mask.fill_path(
        &band,
        tiny_skia::FillRule::EvenOdd,
        true,
        tiny_skia::Transform::identity(),
    );
    let mut pb = tiny_skia::PathBuilder::new();
    for line in dash_polylines(wf, hf, r.min(wf * 0.5).min(hf * 0.5), bw) {
        let mut points = line.into_iter();
        let Some(first) = points.next() else { continue };
        pb.move_to(first.x, first.y);
        for p in points {
            pb.line_to(p.x, p.y);
        }
    }
    let path = pb.finish()?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    let stroke = tiny_skia::Stroke {
        width: bw * 2.0 * 1.1,
        line_cap: tiny_skia::LineCap::Butt,
        ..tiny_skia::Stroke::default()
    };
    pixmap.stroke_path(
        &path,
        &paint,
        &stroke,
        tiny_skia::Transform::identity(),
        Some(&mask),
    );
    Some(ColorImage::from_rgba_premultiplied(
        [w as usize, h as usize],
        pixmap.data(),
    ))
}

type Key = (u32, u32, u32, u32);

#[derive(Clone, Default)]
struct Cache(Arc<Mutex<HashMap<Key, Option<TextureHandle>>>>);

/// Paints a dashed `width` (CSS px) border of `color` on the snapped border
/// box `rect` (points) with corner `radius` (CSS px), moved by `shift`
/// (points) after snapping (a transform).
pub fn paint(
    painter: &Painter,
    rect: Rect,
    radius: f32,
    width: f32,
    color: Rgba,
    shift: egui::Vec2,
) {
    if color.a <= 0.0 {
        return;
    }
    let ctx: &Context = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let min = (rect.min.to_vec2() * ppp).round();
    let max = (rect.max.to_vec2() * ppp).round();
    let (w, h) = ((max.x - min.x) as u32, (max.y - min.y) as u32);
    if w == 0 || h == 0 {
        return;
    }
    let bw = snap::border_device(width, ppp);
    let r = radius * ppp;
    let key: Key = (w, h, (r * 64.0).round() as u32, (bw * 64.0) as u32);
    let cache = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(Id::unique("accshift::dashed-border"))
            .clone()
    });
    let texture = {
        let mut map = cache.0.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key)
            .or_insert_with(|| {
                raster(w, h, r, bw).map(|image| {
                    ctx.load_texture(format!("dashed-{key:?}"), image, TextureOptions::LINEAR)
                })
            })
            .clone()
    };
    let Some(texture) = texture else { return };
    let dest = Rect::from_min_size(
        pos2(min.x / ppp, min.y / ppp) + shift,
        vec2(w as f32 / ppp, h as f32 / ppp),
    );
    painter.image(
        texture.id(),
        dest,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        color.to_color32(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Color32;

    #[test]
    fn the_new_persona_card_takes_blinks_dash_gap() {
        // 152 x 182 device px, radius 14: 643.398 measured, 643 used, 129
        // dashes, the gap shrunk to 1.98450 (the reference's period).
        let segs = rrect_segments(152.0, 182.0, 14.0);
        let length = measure(&segs).last().unwrap().distance;
        assert!((length - 643.398).abs() < 0.01, "{length}");
        let gap = select_best_dash_gap(length.trunc(), 3.0, 2.0);
        assert!((gap - 1.984_496).abs() < 1e-4, "{gap}");
    }

    #[test]
    fn the_first_dash_closes_the_contour() {
        // The last dash runs into the end and continues across the start:
        // it begins a little before x = 14 on the top edge.
        let lines = dash_polylines(152.0, 182.0, 14.0, 1.0);
        let last = lines.last().unwrap();
        let start = last.first().unwrap();
        let end = last.last().unwrap();
        assert!(
            start.y.abs() < 0.05 && (13.5..14.0).contains(&start.x),
            "{start:?}"
        );
        assert!((end.x - 17.0).abs() < 1e-3 && end.y.abs() < 1e-3, "{end:?}");
        assert_eq!(lines.len(), 129);
    }

    #[test]
    fn a_path_of_two_dashes_is_solid() {
        // 6 px around: no longer than two 3 px dashes.
        assert_eq!(dash_polylines(2.0, 1.0, 0.0, 1.0).len(), 1);
        // 10 px around: dashed.
        assert!(dash_polylines(3.0, 2.0, 0.0, 1.0).len() > 1);
    }

    #[test]
    fn white_band_is_opaque_on_straight_edges() {
        let image = raster(152, 182, 14.0, 1.0).unwrap();
        // x = 14..17 on the top row is the joined first dash.
        let at = |x: usize, y: usize| image.pixels[y * 152 + x];
        assert_eq!(at(15, 0), Color32::WHITE);
        assert_eq!(at(15, 1), Color32::TRANSPARENT);
        assert_eq!(at(17, 0), Color32::TRANSPARENT);
    }
}
