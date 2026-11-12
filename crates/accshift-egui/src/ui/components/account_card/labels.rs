//! The text under the avatar: the name line and the meta stack (the
//! username row with its note icon and badge, the inline note, the last
//! login).

use egui::{pos2, vec2, Painter, Rect};

use accshift_app::platforms::card::ChipTone;
use accshift_app::time::format_relative_time_compact;

use super::marquee::NameFit;
use super::measure::{line_box, meta_style, name_style, BADGE, META_GAP, META_TOP, NOTE_ICON};
use super::Card;
use crate::theme::Rgba;
use crate::ui::components::{icon, TransformScope};
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// The gap between the items of the username row.
const ROW_GAP: f32 = 4.0;
/// The badge's colour for a green tone.
const BADGE_GREEN: Rgba = Rgba::new(74.0, 222.0, 128.0, 1.0);

const BADGE_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="8" width="18" height="12" rx="2"/><path d="M8 8V6a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><path d="M3 13h18"/></svg>"##;

/// Paints the name and the meta stack. Under a scale (dragged, entering)
/// they are laid out at their transformed size inside a transform scope.
pub(super) fn paint(card: &Card, painter: &Painter, name: &NameFit) {
    let scope = card.scaled().then(|| {
        let s = card.transform().scale;
        TransformScope::begin(
            &card.ctx,
            painter.layer_id(),
            egui::emath::TSTransform::new(
                card.layout().center().to_vec2() * (1.0 - s) + card.transform().translate,
                s,
            ),
        )
    });
    paint_name(card, painter, name);
    if card.meta.any() {
        paint_meta(card, painter);
    }
    if let Some(scope) = scope {
        scope.end(&card.ctx, Some(painter.clip_rect()));
    }
}

/// The name line. A name that fits, or that no hover found overflowing,
/// is centred, sits on the line's baseline by its bottom edge and ends in
/// an ellipsis when cut. An overflowing one is as wide as its text, sits
/// on its text baseline and starts at the line's start, shifted by the
/// marquee.
fn paint_name(card: &Card, painter: &Painter, name: &NameFit) {
    let lines = &card.lines;
    let content_left = card.content_left();
    let content_w = card.content_w();
    let name_top = card.name_top();
    let name_box = Rect::from_min_size(
        pos2(content_left, name_top),
        vec2(content_w, lines.strut.height),
    );
    let (inner, ellipsis, clip) = if name.overflowing {
        let top = name_top + lines.strut.baseline - lines.name.baseline;
        let inner = Rect::from_min_size(
            pos2(content_left + name.offset, top),
            vec2(name.width, lines.name.height),
        );
        (inner, false, name_box)
    } else {
        let w = name.width.min(content_w);
        let top = name_top + lines.strut.baseline - lines.name.height;
        let inner = Rect::from_min_size(
            pos2(content_left + snap::centre(content_w, w, card.ppp), top),
            vec2(w, lines.name.height),
        );
        (inner, name.width > content_w, inner.intersect(name_box))
    };
    let mut name_painter = painter.clone();
    name_painter.set_clip_rect(painter.clip_rect().intersect(card.follow(clip)));
    let style = name_style(card.v()).nowrap(ellipsis).align(TextAlign::Left);
    text::paint(&name_painter, card.follow(inner), &card.item().name, &style);
}

/// The meta stack under the name, its lines 2 px apart.
fn paint_meta(card: &Card, painter: &Painter) {
    let (v, item, lines, meta) = (card.v(), card.item(), &card.lines, &card.meta);
    let content_left = card.content_left();
    let content_w = card.content_w();
    let line = |y: f32, h: f32| Rect::from_min_size(pos2(content_left, y), vec2(content_w, h));
    let mut y = card.name_top() + lines.strut.height + META_TOP;
    if meta.row() {
        let row_h = meta.row_height(lines);
        paint_username_row(card, painter, line(y, row_h));
        y += row_h + META_GAP;
    }
    if meta.note {
        let style = meta_style(500, 9.0, v.fg)
            .nowrap(true)
            .align(TextAlign::Center);
        let r = card.follow(line(y, lines.note.height));
        text::paint(painter, r, item.note.trim(), &style);
        y += lines.note.height + META_GAP;
    }
    if meta.last_login {
        let label = format_relative_time_compact(
            item.last_login_at_sec,
            card.props.time,
            item.last_login_unknown_key,
        );
        let style = meta_style(500, 10.0, v.fg_subtle.mix(0.4, v.fg))
            .nowrap(true)
            .align(TextAlign::Center);
        text::paint(
            painter,
            card.follow(line(y, lines.meta.height)),
            &label,
            &style,
        );
    }
}

/// The username row: the note icon, the username (shrinking, with an
/// ellipsis) and the badge, centred, 4 px apart.
fn paint_username_row(card: &Card, painter: &Painter, row: Rect) {
    let meta = &card.meta;
    let ppp = card.ppp;
    let fixed: f32 = [meta.icon.then_some(NOTE_ICON), meta.badge.then_some(BADGE)]
        .into_iter()
        .flatten()
        .sum();
    let count =
        usize::from(meta.icon) + usize::from(meta.username.is_some()) + usize::from(meta.badge);
    let gaps = ROW_GAP * count.saturating_sub(1) as f32;
    let text_w = meta
        .username
        .map(|w| w.min((row.width() - fixed - gaps).max(0.0)));
    let total = fixed + gaps + text_w.unwrap_or(0.0);
    let mut x = row.left() + snap::centre(row.width(), total, ppp);
    if meta.icon {
        paint_note_icon(card, painter, row, x);
        x += NOTE_ICON + ROW_GAP;
    }
    if let (Some(w), Some(full)) = (text_w, meta.username) {
        let h = card.lines.meta.height;
        let r = Rect::from_min_size(
            pos2(x, row.top() + snap::centre(row.height(), h, ppp)),
            vec2(w, h),
        );
        let style = meta_style(400, 10.0, card.v().fg_muted)
            .nowrap(full > w)
            .align(TextAlign::Center);
        text::paint(painter, card.follow(r), &card.item().username, &style);
        x += w + ROW_GAP;
    }
    if meta.badge {
        paint_badge(card, painter, row, x);
    }
}

/// The note icon at `x` in the row: an "i" in a 1 px circle.
fn paint_note_icon(card: &Card, painter: &Painter, row: Rect, x: f32) {
    let v = card.v();
    let ppp = card.ppp;
    let r = Rect::from_min_size(
        pos2(x, row.top() + snap::centre(row.height(), NOTE_ICON, ppp)),
        vec2(NOTE_ICON, NOTE_ICON),
    );
    let placed = card.follow(r);
    let bw = snap::border_width(1.0, ppp);
    BoxStyle {
        radius: Corners::all(NOTE_ICON / 2.0),
        border: Border::all(bw, v.fg.mix(0.92, Rgba::TRANSPARENT)),
        ..BoxStyle::default()
    }
    .paint(painter, snap::rect(placed, ppp));
    let style = TextStyle::new(8.0, 700)
        .line_height(LineHeight::Number(1.0))
        .nowrap(false)
        .align(TextAlign::Center)
        .color(v.fg.to_color32());
    let lb = line_box(&card.ctx, &style).height;
    let content = NOTE_ICON - 2.0 * bw;
    let inner = Rect::from_min_size(
        pos2(
            placed.left() + bw,
            placed.top() + bw + snap::centre(content, lb, ppp),
        ),
        vec2(content, lb),
    );
    text::paint(painter, inner, "i", &style);
}

/// The platform's badge at `x` in the row, green or in the subtle
/// foreground.
fn paint_badge(card: &Card, painter: &Painter, row: Rect, x: f32) {
    let Some(badge) = card.props.badge else {
        return;
    };
    let color = match badge.tone {
        ChipTone::Green => BADGE_GREEN,
        _ => card.v().fg_subtle.mix(0.75, Rgba::TRANSPARENT),
    };
    let r = Rect::from_min_size(
        pos2(x, row.top() + snap::centre(row.height(), BADGE, card.ppp)),
        vec2(BADGE, BADGE),
    );
    icon::paint(painter, BADGE_SVG, card.follow(r), color, 1.0);
}
