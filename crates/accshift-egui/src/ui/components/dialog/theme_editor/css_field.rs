use std::ops::Range;

use egui::{Event, Pos2};

use super::super::*;
use super::layout::line_h;
use super::{Styles, CSS_PAD};
use crate::ui::components::controls::editing::{self, Editor, Rows, Rules};
use crate::ui::components::controls::field_menu;
use crate::ui::components::controls::text_input::settings_style;
use crate::ui::components::controls::{paint_ua_focus_ring, scroll};

/// The custom CSS `<textarea>`: wraps, scrolls past six rows and edits as
/// Chromium does ([`editing`] with [`Rules::area`]). Returns whether the
/// text changed.
pub(super) fn css_field(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    text: &mut String,
    scroll_top: &mut f32,
    s: &Styles,
    props: &DialogProps,
) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let boxed = snap::rect(rect, ppp);
    // Dragging selects.
    let response = ui
        .interact(boxed, id, Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::Text);
    let focused = take_focus(ui, &response);
    field_menu::on_response(&ctx, id, &response);
    let b = snap::border_width(1.0, ppp);
    let viewport = Rect::from_min_max(
        pos2(rect.left() + b, rect.top() + b),
        pos2(rect.right() - b, rect.bottom() - b),
    );
    let line = line_h(&ctx, &s.mono);
    let rules = Rules::area(accshift_theme::MAX_THEME_CSS_LENGTH);
    let mut ed = Editor::load(&ctx, id, text);
    let mut changed = false;
    let mut moved = false;
    let mut page = false;
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            )
        });
        let before = css_layout(&ctx, text, &ed, s, viewport, *scroll_top, true);
        let rows = before.rows.paging(viewport.height(), ppp);
        let caret = (ed.caret, ed.anchor, ed.upstream);
        ed.area_pointer(ui, &response, text, &rules, &rows);
        let mut events: Vec<Event> = ui.input(|i| i.events.clone());
        events.extend(field_menu::take_event(&ctx, id));
        let out = ed.area_keys(&ctx, text, &events, &rules, &rows);
        changed = out.changed;
        page = out.page;
        moved = changed || page || (ed.caret, ed.anchor, ed.upstream) != caret;
        if moved {
            // Chromium restarts the blink on every caret move.
            let now = ui.input(|i| i.time);
            ctx.data_mut(|d| d.insert_temp(id.with("edit-at"), now));
        }
    } else {
        ed.blur();
    }
    let mut laid = css_layout(&ctx, text, &ed, s, viewport, *scroll_top, focused);
    // Blink's caret (as in the single-line fields): 1 CSS px wide, over the
    // text fragment (rounded ascent plus descent under the floored
    // half-leading).
    let (lead, height) = s.mono.fragment(&ctx).unwrap_or((0.0, line));
    if moved {
        // `RevealSelection`: the caret into the scrollport, to the nearest
        // edge, or to the top after a page move.
        let (row, _) = laid.rows.locate(laid.caret, ed.upstream);
        let y = CSS_PAD + row as f32 * line + lead;
        let mut top = if page { y } else { *scroll_top };
        if y + height > top + viewport.height() {
            top = y + height - viewport.height();
        }
        if y < top {
            top = y;
        }
        let max = (laid.content - viewport.height()).max(0.0);
        let top = ((top.clamp(0.0, max)) * ppp).round() / ppp;
        if top != *scroll_top {
            *scroll_top = top;
            laid = css_layout(&ctx, text, &ed, s, viewport, top, focused);
        }
    }
    let field = settings_style(v);
    BoxStyle {
        background: v.bg_input,
        radius: Corners::all(v.radius_sm),
        border: Border::all(b, v.border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    let painter = ui
        .painter()
        .with_clip_rect(snap::rect(viewport, ppp).intersect(ui.clip_rect()));
    match &laid.laid {
        None => {
            text::paint(
                &painter,
                laid.text_rect,
                ".account-card { letter-spacing: 0.02em; }",
                &s.mono.clone().color(field.placeholder.to_color32()),
            );
        }
        Some(l) => {
            if focused {
                // `::selection { background: color-mix(--accent 45%,
                // transparent) }`; a selected line break is a space wide.
                let space = text::measure(&ctx, " ", &s.mono).unwrap_or(0.0);
                let color = v.accent.mix(0.45, Rgba::TRANSPARENT).to_color32();
                for (row, x1, x2) in laid.rows.spans(laid.selection.clone(), space) {
                    let top = laid.rows.top(row);
                    let sel = Rect::from_x_y_ranges(x1..=x2, top..=top + line);
                    painter.rect_filled(snap::rect(sel, ppp), 0.0, color);
                }
            }
            l.paint(&painter);
            if focused {
                // The composition's underline: one device px at the bottom
                // of its line boxes.
                for (row, x1, x2) in laid.rows.spans(laid.preedit.clone(), 0.0) {
                    let y = snap::coord(laid.rows.top(row) + line, ppp) - 1.0 / ppp;
                    let under = Rect::from_x_y_ranges(x1..=x2, y..=y + 1.0 / ppp);
                    painter.rect_filled(under, 0.0, v.fg.to_color32());
                }
            }
        }
    }
    scroll::scroll(ui, viewport, id.with("scroll"), scroll_top, laid.content, v);
    paint_resizer(&ui.painter().with_clip_rect(ui.clip_rect()), boxed, ppp);
    if focused {
        // After text the caret is centred on its offset and rounded, on an
        // empty row it starts at the floored offset.
        let (row, x) = laid.rows.locate(laid.caret, ed.upstream);
        let line_top = snap::layout_unit(laid.rows.top(row) * ppp);
        let y = (line_top + lead * ppp + 0.5).floor();
        let caret_w = ppp.max(1.0);
        let start = snap::layout_unit(laid.text_rect.left() * ppp);
        let at = (((x - laid.text_rect.left()).max(0.0)) * ppp * 64.0).floor() / 64.0;
        let (left, right) = if laid.rows.blank(row) {
            let left = (start + at).floor();
            (left, (left + caret_w + 0.5).floor())
        } else {
            let x = start + ((at - caret_w / 2.0).max(0.0) + 0.5).floor();
            ((x + 0.5).floor(), (x + caret_w + 0.5).floor())
        };
        let caret = Rect::from_min_size(
            pos2(left, y) / ppp,
            vec2((right - left).max(1.0), (height * ppp).round()) / ppp,
        );
        editing::ime(&ctx, boxed, caret);
        if laid.selection.is_empty() {
            let now = ui.input(|i| i.time);
            let since = now - ctx.data(|d| d.get_temp::<f64>(id.with("edit-at")).unwrap_or(now));
            let phase = (since / 0.5).floor() as i64;
            if phase % 2 == 0 {
                painter.rect_filled(caret, 0.0, v.fg.to_color32());
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                (0.5 * (phase + 1) as f64 - since).max(0.01),
            ));
        }
        // `textarea` takes `:focus-visible` from the mouse too.
        paint_ua_focus_ring(ui.painter(), boxed, v.radius_sm, 1.0, props.dark);
    }
    ed.store(&ctx, id, text);
    changed
}

/// The textarea's text laid out for a frame.
struct CssLayout {
    /// `None` while the text is empty (the placeholder shows).
    laid: Option<text::TextLayout>,
    rows: Rows,
    /// The first line box, scrolled.
    text_rect: Rect,
    /// The scroll height: the lines and the padding.
    content: f32,
    /// The caret, selection and composition in the laid out text.
    caret: usize,
    selection: Range<usize>,
    preedit: Range<usize>,
}

/// Lays `text` out in the textarea's content box under `scroll_top`, the
/// composition at the caret while focused, narrowed by the scrollbar when
/// the lines overflow.
fn css_layout(
    ctx: &Context,
    text: &str,
    ed: &Editor,
    s: &Styles,
    viewport: Rect,
    scroll_top: f32,
    focused: bool,
) -> CssLayout {
    let line = line_h(ctx, &s.mono);
    let pre = if focused { ed.preedit.as_str() } else { "" };
    let caret = ed.caret.min(text.chars().count());
    let split = text
        .char_indices()
        .nth(caret)
        .map_or(text.len(), |(i, _)| i);
    let shown = format!("{}{pre}{}", &text[..split], &text[split..]);
    let pre_len = pre.chars().count();
    let top = viewport.top() + CSS_PAD - scroll_top;
    let lay = |w: f32| {
        let r = Rect::from_min_size(pos2(viewport.left() + CSS_PAD, top), vec2(w, line));
        let laid = if shown.is_empty() {
            None
        } else {
            text::layout(ctx, &shown, &s.mono, r)
        };
        let h = laid.as_ref().map_or(line, |l| l.height.max(line));
        (r, laid, h + 2.0 * CSS_PAD)
    };
    let mut inner_w = viewport.width() - 2.0 * CSS_PAD;
    let (mut text_rect, mut laid, mut content) = lay(inner_w);
    if scroll::overflows(viewport.height(), content) {
        inner_w -= scroll::WIDTH;
        (text_rect, laid, content) = lay(inner_w);
    }
    let shift = |i: usize| if i > caret { i + pre_len } else { i };
    let sel = ed.selection();
    let preedit = caret..caret + pre_len;
    CssLayout {
        rows: Rows::new(laid.as_ref(), text_rect.min, line, preedit.clone()),
        laid,
        text_rect,
        content,
        caret: caret + pre_len,
        selection: shift(sel.start)..shift(sel.end),
        preedit,
    }
}

/// Blink's textarea resizer (`DrawPlatformResizerImage`): two diagonals in
/// a corner the size of a classic 17 px scrollbar, a 60 % black line with a
/// 60 % white copy `e` device px under it, non-antialiased. The corner
/// offsets and the bottom cut were fitted on Chromium captures at 1x and
/// 1.5x.
fn paint_resizer(painter: &egui::Painter, boxed: Rect, ppp: f32) {
    let b = (snap::border_width(1.0, ppp) * ppp).round();
    let pad_r = (boxed.right() * ppp).round() - b;
    let pad_b = (boxed.bottom() * ppp).round() - b;
    let size = (17.0 * ppp).floor();
    let e = ppp.ceil();
    let right = pad_r - ppp;
    let bottom = pad_b - (ppp - 1.0);
    let (x, y) = (right - size, bottom - size);
    let half = (size / 2.0).floor();
    let three = (size * 3.0 / 4.0).floor();
    let lines = [
        [pos2(right - e, y + half), pos2(x + half, bottom - e)],
        [pos2(right - e, y + three), pos2(x + three, bottom - e)],
    ];
    for (dy, color) in [
        (0.0, Color32::from_black_alpha(153)),
        (e, Color32::from_white_alpha(153)),
    ] {
        // A mesh, not `rect_filled`: epaint turns 1 px squares into line
        // segments too short to draw.
        let mut mesh = egui::Mesh::default();
        for [a, c] in lines {
            stroke_rows(
                &mut mesh,
                a + vec2(0.0, dy),
                c + vec2(0.0, dy),
                e,
                pad_b - 2.0,
                color,
                ppp,
            );
        }
        painter.add(mesh);
    }
}

/// Fills a butt-capped segment the way Skia's non-AA scan converter does:
/// a row is in when its centre is, a pixel when its centre falls in the
/// rounded span. Coordinates in device px; rows from `clip` down are cut.
fn stroke_rows(
    mesh: &mut egui::Mesh,
    a: Pos2,
    c: Pos2,
    width: f32,
    clip: f32,
    color: Color32,
    ppp: f32,
) {
    let d = c - a;
    let n = vec2(-d.y, d.x) / d.length() * (width / 2.0);
    let quad = [a + n, c + n, c - n, a - n];
    let top = quad
        .iter()
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min)
        .floor();
    let bottom = quad
        .iter()
        .map(|p| p.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(clip);
    let mut row = top;
    while row < bottom {
        let cy = row + 0.5;
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for (i, p) in quad.iter().enumerate() {
            let q = quad[(i + 1) % quad.len()];
            if p.y == q.y || cy < p.y.min(q.y) || cy >= p.y.max(q.y) {
                continue;
            }
            let x = p.x + (cy - p.y) * (q.x - p.x) / (q.y - p.y);
            lo = lo.min(x);
            hi = hi.max(x);
        }
        let (l, r) = ((lo + 0.5).floor(), (hi + 0.5).floor());
        if r > l {
            mesh.add_colored_rect(
                Rect::from_min_max(pos2(l, row) / ppp, pos2(r, row + 1.0) / ppp),
                color,
            );
        }
        row += 1.0;
    }
}
