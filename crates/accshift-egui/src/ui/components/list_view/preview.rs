//! The preview panel on the right: the selected account's avatar, name,
//! meta lines, warning chips, note, active status and switch button in a
//! centred column, or a hint when no account is selected.

use accshift_app::display::AccountCardItem;
use accshift_app::i18n::translate;
use accshift_app::platforms::warnings::WarningTone;
use egui::{pos2, vec2, Id, Painter, Pos2, Rect, Sense, Ui};

use super::avatar::{paint_avatar, AvatarSize};
use super::{
    ceil_unit, has_username, last_login_color, last_login_label, meta_style, ListEvent, ListHost,
    ListProps, ORANGE, RADIUS, RED, T_BUTTON,
};
use crate::app::vars::Vars;
use crate::icons;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::components::account_card::AvatarContent;
use crate::ui::components::dialog::auto_ring;
use crate::ui::components::icon;
use crate::ui::css;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle, TextTransform};

/// The avatar's side before any shrink.
const AVATAR: f32 = 120.0;
/// A meta line's height: 12 px text, line height 1.2.
const META_LINE: f32 = 12.0 * 1.2;

/// Draws the preview over `rect`: the `selected` account's, or the hint.
/// Reports a switch when its button is pressed.
pub(super) fn preview(
    ui: &mut Ui,
    rect: Rect,
    selected: Option<&AccountCardItem>,
    props: &ListProps,
    host: &mut dyn ListHost,
) -> Option<ListEvent> {
    let ppp = ui.ctx().pixels_per_point();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let Some(a) = selected else {
        no_selection(&child, rect, props);
        return None;
    };
    let key = format!("list/preview/{}", a.account.id);
    let id = host.mount(&key);
    let px = (AVATAR * ppp).round() as u32;
    let content = host.avatar(&key, a, px, true);
    let color = host.card_color(&a.card_color);
    let pressed = preview_panel(&mut child, rect, id, a, &content, color, props);
    (pressed && !a.pending_setup).then(|| ListEvent::Switch(a.account.id.clone()))
}

/// The hint with no account selected, centred both ways in a 16 px padding.
fn no_selection(ui: &Ui, rect: Rect, props: &ListProps) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(v.line_height))
        .align(TextAlign::Center)
        .color(v.fg_muted.to_color32());
    let inner = rect.shrink(16.0);
    let text = translate(props.locale, "list.selectAccountPreview");
    let probe = Rect::from_min_size(inner.min, vec2(inner.width(), 0.0));
    let h = text::layout(ctx, &text, &style, probe).map_or(11.0 * v.line_height, |l| l.height);
    let top = inner.top() + snap::centre(inner.height(), h, ppp);
    text::paint(
        ui.painter(),
        Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), h)),
        &text,
        &style,
    );
}

/// The note the preview shows: a blank one shows no block, as on the card.
pub(super) fn preview_note(a: &AccountCardItem) -> Option<&str> {
    Some(a.note.trim()).filter(|n| !n.is_empty())
}

/// Blink's flex shrink of the preview column, for its two items that can
/// shrink (bases `a` and `b`, minimum 0): each gives way in proportion to its
/// base, `LayoutUnit::FromFloatRound` on the device length.
pub(super) fn flex_shrink(overflow: f32, a: f32, b: f32, ppp: f32) -> (f32, f32) {
    let sum = a + b;
    if overflow <= 0.0 || sum <= 0.0 {
        return (a, b);
    }
    let give = |base: f32| {
        let shrunk = base - overflow.min(sum) * base / sum;
        snap::layout_unit(shrunk * ppp) / ppp
    };
    (give(a), give(b))
}

/// The preview's column: its content box, where it paints, at which scale.
struct PreviewColumn {
    painter: Painter,
    inner: Rect,
    ppp: f32,
}

impl PreviewColumn {
    /// A full width band from `top`, `h` tall.
    fn at(&self, top: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(self.inner.left(), top), vec2(self.inner.width(), h))
    }

    /// The left edge of a box `w` wide centred in the column.
    fn centre_x(&self, w: f32) -> f32 {
        self.inner.left() + snap::centre(self.inner.width(), w, self.ppp)
    }
}

/// The height of `s` wrapped in `w`.
fn wrapped_height(ctx: &egui::Context, s: &str, style: &TextStyle, w: f32) -> Option<f32> {
    let probe = Rect::from_min_size(Pos2::ZERO, vec2(w, 0.0));
    text::layout(ctx, s, style, probe).map(|l| l.height)
}

/// A card colour tints the panel: a vertical gradient from 14 % of it over
/// the card background to 8 % of it.
fn paint_tint(painter: &Painter, rect: Rect, c: Rgba, v: &Vars, ppp: f32) {
    let css_rgba = |c: Rgba| format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, c.a);
    let gradient = css::gradient(&format!(
        "linear-gradient(180deg, {}, {})",
        css_rgba(c.mix(0.14, v.bg_card)),
        css_rgba(c.mix(0.08, Rgba::TRANSPARENT)),
    ));
    BoxStyle {
        images: gradient.into_iter().collect(),
        radius: Corners::new(0.0, RADIUS, RADIUS, 0.0),
        ..BoxStyle::default()
    }
    .paint(painter, snap::rect(rect, ppp));
}

/// The warning chips: wrapping lines, 4 px apart, centred.
struct Chips {
    style: TextStyle,
    /// Each chip's tone, text and width.
    items: Vec<(WarningTone, String, f32)>,
    /// Each line's chips, by index.
    lines: Vec<Vec<usize>>,
    /// One chip's height.
    h: f32,
}

impl Chips {
    fn measure(ctx: &egui::Context, a: &AccountCardItem, props: &ListProps, width: f32) -> Chips {
        let ppp = ctx.pixels_per_point();
        let v = props.vars;
        let style = TextStyle::new(9.0, 700)
            .line_height(LineHeight::Number(v.line_height))
            .letter_spacing_px(0.3)
            .transform(TextTransform::Uppercase);
        let items: Vec<(WarningTone, String, f32)> = a
            .warning
            .as_ref()
            .map(|w| {
                w.chips
                    .iter()
                    .map(|chip| {
                        let text = chip.text.resolve(props.locale);
                        let w = text::measure(ctx, &text, &style).unwrap_or(0.0);
                        (chip.tone, text, ceil_unit(w, ppp) + 12.0)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut lines: Vec<Vec<usize>> = Vec::new();
        let mut line_w = 0.0;
        for (i, (_, _, w)) in items.iter().enumerate() {
            match lines.last_mut() {
                Some(line) if line_w + 4.0 + w <= width => {
                    line.push(i);
                    line_w += 4.0 + w;
                }
                _ => {
                    lines.push(vec![i]);
                    line_w = *w;
                }
            }
        }
        Chips {
            style,
            items,
            lines,
            h: 9.0 * v.line_height + 4.0,
        }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn height(&self) -> f32 {
        match self.lines.len() {
            0 => 0.0,
            n => n as f32 * self.h + (n - 1) as f32 * 4.0,
        }
    }

    /// Paints the chips from `y`; returns the bottom of the last line.
    fn paint(&self, col: &PreviewColumn, mut y: f32) -> f32 {
        for (n, line) in self.lines.iter().enumerate() {
            if n > 0 {
                y += self.h + 4.0;
            }
            let line_w =
                line.iter().map(|&i| self.items[i].2).sum::<f32>() + 4.0 * (line.len() - 1) as f32;
            let mut x = col.centre_x(line_w);
            for &i in line {
                let (tone, text, w) = &self.items[i];
                let (bg, fg) = match tone {
                    WarningTone::Red => (RED.alpha(0.2), Rgba::hex(0xf87171, 1.0)),
                    WarningTone::Orange => (ORANGE.alpha(0.2), Rgba::hex(0xfb923c, 1.0)),
                };
                let chip = Rect::from_min_size(pos2(x, y), vec2(*w, self.h));
                BoxStyle {
                    background: bg,
                    radius: Corners::all(3.0),
                    ..BoxStyle::default()
                }
                .paint(&col.painter, snap::rect(chip, col.ppp));
                text::paint(
                    &col.painter,
                    Rect::from_min_size(chip.min + vec2(6.0, 2.0), vec2(w - 12.0, self.h - 4.0)),
                    text,
                    &self.style.clone().color(fg.to_color32()),
                );
                x += w + 4.0;
            }
        }
        y + self.h
    }
}

/// The note block: padding 8 by 10 inside a border, the label then the
/// note.
struct Note<'a> {
    text: &'a str,
    text_style: TextStyle,
    text_h: f32,
    label_style: TextStyle,
    label_h: f32,
    /// The text's width.
    w: f32,
    bw: f32,
    /// The block's height.
    h: f32,
}

impl<'a> Note<'a> {
    fn measure(ctx: &egui::Context, a: &'a AccountCardItem, v: &Vars, width: f32) -> Option<Self> {
        let text = preview_note(a)?;
        let bw = snap::border_width(v.border_px(), ctx.pixels_per_point());
        let label_style = TextStyle::new(10.0, 700)
            .line_height(LineHeight::Number(v.line_height))
            .letter_spacing_em(0.06)
            .transform(TextTransform::Uppercase)
            .color(v.fg_muted.to_color32());
        let text_style = TextStyle::new(12.0, 400)
            .line_height(LineHeight::Number(1.35))
            .color(v.fg.to_color32());
        let w = width - 20.0 - 2.0 * bw;
        let text_h = wrapped_height(ctx, text, &text_style, w).unwrap_or(12.0 * 1.35);
        let label_h = 10.0 * v.line_height;
        let h = 16.0 + 2.0 * bw + label_h + 4.0 + text_h;
        Some(Note {
            text,
            text_style,
            text_h,
            label_style,
            label_h,
            w,
            bw,
            h,
        })
    }

    fn paint(&self, col: &PreviewColumn, block: Rect, v: &Vars, props: &ListProps) {
        BoxStyle {
            background: v.bg_muted.mix(0.74, Rgba::TRANSPARENT),
            radius: Corners::all(6.0),
            border: Border::all(self.bw, v.border),
            ..BoxStyle::default()
        }
        .paint(&col.painter, snap::rect(block, col.ppp));
        let left = block.left() + self.bw + 10.0;
        let mut top = block.top() + self.bw + 8.0;
        text::paint(
            &col.painter,
            Rect::from_min_size(pos2(left, top), vec2(self.w, self.label_h)),
            &translate(props.locale, "preview.noteLabel"),
            &self.label_style,
        );
        top += self.label_h + 4.0;
        text::paint(
            &col.painter,
            Rect::from_min_size(pos2(left, top), vec2(self.w, self.text_h)),
            self.text,
            &self.text_style,
        );
    }
}

/// The preview of `a`: true when its switch button was pressed.
fn preview_panel(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    a: &AccountCardItem,
    content: &AvatarContent,
    card_color: Option<Rgba>,
    props: &ListProps,
) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let painter = ui.painter().clone();
    if let Some(c) = card_color {
        paint_tint(&painter, rect, c, v, ppp);
    }

    // The column, centred in the padding box.
    let col = PreviewColumn {
        painter,
        inner: Rect::from_min_max(rect.min + vec2(16.0, 24.0), rect.max - vec2(16.0, 24.0)),
        ppp,
    };
    let inner = col.inner;
    let width = inner.width();
    let name_style = TextStyle::new(15.0, 600)
        .line_height(LineHeight::Number(v.line_height))
        .align(TextAlign::Center)
        .color(v.fg.to_color32());
    let name_h = wrapped_height(&ctx, &a.name, &name_style, width).unwrap_or(15.0 * v.line_height);
    let last_login = last_login_label(a, props.time);
    let meta_lines: Vec<(&str, TextStyle)> = [
        (
            has_username(a).then_some(a.account.username.as_str()),
            meta_style(12.0, 400, v.fg_muted),
        ),
        (
            last_login.as_deref(),
            meta_style(12.0, 500, last_login_color(v)),
        ),
    ]
    .into_iter()
    .filter_map(|(line, style)| Some((line?, style.align(TextAlign::Center))))
    .collect();
    let chips = Chips::measure(&ctx, a, props, width);
    let note = Note::measure(&ctx, a, v, width);
    let status_h = 11.0 * v.line_height;
    let button_h = 16.0 + 12.0 * v.line_height;

    // Each block present adds its gap above and its height.
    let meta_h = meta_lines.len() as f32 * META_LINE;
    let mut total = AVATAR + 16.0 + name_h;
    for (shown, gap, h) in [
        (!meta_lines.is_empty(), 4.0, meta_h),
        (!chips.is_empty(), 6.0, chips.height()),
        (note.is_some(), 10.0, note.as_ref().map_or(0.0, |n| n.h)),
        (a.active, 16.0, status_h),
        (!a.pending_setup, 16.0, button_h),
    ] {
        if shown {
            total += gap + h;
        }
    }
    // Past the column's height every block shrinks in proportion to its
    // size, but only the avatar and the meta lines can: they clip their
    // overflow, so their minimum height is 0.
    let (avatar_h, meta_box_h) = flex_shrink(total - inner.height(), AVATAR, meta_h, ppp);
    total -= (AVATAR - avatar_h) + (meta_h - meta_box_h);
    let mut y = inner.top() + snap::centre(inner.height(), total, ppp);

    let avatar = Rect::from_min_size(pos2(col.centre_x(AVATAR), y), vec2(AVATAR, avatar_h));
    paint_avatar(ui, id, avatar, content, a, v, AvatarSize::PREVIEW);
    y += avatar_h + 16.0;
    text::paint(&col.painter, col.at(y, name_h), &a.name, &name_style);
    y += name_h;
    if !meta_lines.is_empty() {
        y += 4.0;
        let box_clip = snap::rect(col.at(y, meta_box_h), ppp);
        let clipped = col
            .painter
            .with_clip_rect(col.painter.clip_rect().intersect(box_clip));
        let mut line_y = y;
        for (line, style) in &meta_lines {
            text::paint(&clipped, col.at(line_y, META_LINE), line, style);
            line_y += META_LINE;
        }
        y += meta_box_h;
    }
    if !chips.is_empty() {
        y += 6.0;
        y = chips.paint(&col, y);
    }
    if let Some(note) = &note {
        y += 10.0;
        note.paint(&col, col.at(y, note.h), v, props);
        y += note.h;
    }
    if a.active {
        y += 16.0;
        let style = TextStyle::new(11.0, 600)
            .line_height(LineHeight::Number(v.line_height))
            .letter_spacing_px(0.5)
            .transform(TextTransform::Uppercase)
            .align(TextAlign::Center)
            .color(v.fg_muted.to_color32());
        text::paint(
            &col.painter,
            col.at(y, status_h),
            &translate(props.locale, "preview.currentlyActive"),
            &style,
        );
        y += status_h;
    }
    if a.pending_setup {
        return false;
    }
    switch_button(ui, &col, id, a.active, props, y + 16.0)
}

/// The switch button from `top`, centred: padding 8 by 16, the 14 px play
/// icon, 6 px, the label. Hover brightens it, a press darkens it. True when
/// clicked.
fn switch_button(
    ui: &mut Ui,
    col: &PreviewColumn,
    id: Id,
    active: bool,
    props: &ListProps,
    top: f32,
) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = col.ppp;
    let v = props.vars;
    let painter = &col.painter;
    let button_h = 16.0 + 12.0 * v.line_height;
    let label_style = TextStyle::new(12.0, 500)
        .line_height(LineHeight::Number(v.line_height))
        .nowrap(true);
    let key = if active {
        "preview.switchAgain"
    } else {
        "preview.switchAccount"
    };
    let label = translate(props.locale, key);
    let label_w = ceil_unit(
        text::measure(&ctx, &label, &label_style).unwrap_or(0.0),
        ppp,
    );
    let w = 16.0 + 14.0 + 6.0 + label_w + 16.0;
    let r = Rect::from_min_size(pos2(col.centre_x(w), top), vec2(w, button_h));
    let painted = snap::rect(r, ppp);
    let response = ui
        .interact(painted, id.with("switch"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let brightness = if response.is_pointer_button_down_on() {
        0.9
    } else if response.hovered() {
        1.15
    } else {
        1.0
    };
    let brightness = anim::transition(&ctx, id.with("switch-filter"), brightness, &T_BUTTON);
    let bright = |c: Rgba| {
        let ch = |x: f32| (x * brightness).min(255.0);
        Rgba::new(ch(c.r), ch(c.g), ch(c.b), c.a)
    };
    BoxStyle {
        background: bright(props.accent),
        radius: Corners::all(6.0),
        ..BoxStyle::default()
    }
    .paint(painter, painted);
    let content_h = 12.0 * v.line_height;
    let icon_rect = Rect::from_min_size(
        pos2(
            r.left() + 16.0,
            r.top() + 8.0 + snap::centre(content_h, 14.0, ppp),
        ),
        vec2(14.0, 14.0),
    );
    let white = bright(Rgba::new(255.0, 255.0, 255.0, 1.0));
    icon::paint(painter, icons::PLAY, icon_rect, white, 1.0);
    text::paint(
        painter,
        Rect::from_min_size(
            pos2(icon_rect.right() + 6.0, r.top() + 8.0),
            vec2(label_w, content_h),
        ),
        &label,
        &label_style.color(white.to_color32()),
    );
    if response.has_focus() && props.focus_visible {
        auto_ring(painter, painted, 6.0, props.dark, ppp);
    }
    response.clicked()
}
