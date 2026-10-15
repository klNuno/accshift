//! `CommandPalette.svelte`, mounted by `App.svelte` above `AppDialogs`:
//!
//! ```text
//! .palette-overlay   fixed, inset 0, flex, centred, top aligned,
//!                    color-mix(--bg 45%), backdrop blur(--overlay-blur),
//!                    overlay-fade-in 120ms ease-out
//!   .palette         margin-top 12vh, min(520px, 100vw - 48px) wide,
//!                    --bg-card under --card-bg-image, --border, radius 10,
//!                    0 18px 48px color-mix(--bg 60%), overflow hidden,
//!                    dialog-slide-in 140ms ease-out
//!     input          14 px, padding 13 16, a 1 px
//!                    --border bottom; placeholder --fg-subtle
//!     .palette-list  max-height 46vh, overflow-y auto, padding 6
//!       .palette-section  10 px 600, 0.08em, uppercase, --fg-subtle,
//!                         padding 8 10 4
//!       .palette-item     a flex button: 13 px, padding 8 10, radius 6,
//!                         gap 8; the dot, the title (flex 1, ellipsis),
//!                         the Active badge, the shortcut hint
//!       .palette-empty    padding 22 12, centred, --fg-muted 13 px
//! ```
//!
//! Every line, the field's and the buttons' included, keeps the inherited
//! 1.5 (measured on WebView2: a 48 px field, 35.5 px items, a 19 px badge).
//!
//! Keys on the field (its own `keydown`): ArrowUp and ArrowDown move the
//! highlight with wrap-around, Home and End go to the ends while the query
//! is empty, Enter runs the highlighted command. Escape and Ctrl+K reach
//! the app's binding table, which closes the palette. A mousedown on the
//! overlay outside the box closes it; hovering an item highlights it.

use egui::emath::TSTransform;
use egui::{pos2, vec2, Event, FocusDirection, Id, Key, Painter, Rect, Sense, Ui, UiBuilder};

use accshift_app::actions::palette::{
    CommandPalette, PaletteAction, PaletteCommand, PaletteRow, PaletteSection,
};
use accshift_app::i18n::{self, Locale};

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing};
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::controls::scroll::{self, nearest};
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle, TextTransform};

/// The palette's root id, `.with(mount)`.
pub const PALETTE_ID: &str = "accshift-command-palette";
/// `width: min(520px, calc(100vw - 48px))`.
const WIDTH: f32 = 520.0;
/// `overlay-fade-in 120ms ease-out`.
const FADE: Animation = Animation::new(0.12, Timing::EASE_OUT);
/// `dialog-slide-in 140ms ease-out`.
const SLIDE: Animation = Animation::new(0.14, Timing::EASE_OUT);
/// Chromium's caret blink half period.
const CARET_BLINK: f64 = 0.5;
/// `.palette-input { padding: 13px 16px }`.
const INPUT_PAD: (f32, f32) = (16.0, 13.0);
/// `.palette-list { padding: 6px }`.
const LIST_PAD: f32 = 6.0;
/// `.palette-section { padding: 8px 10px 4px }`.
const SECTION_PAD: (f32, f32, f32) = (10.0, 8.0, 4.0);
/// `.palette-item { padding: 8px 10px; gap: 8px }`.
const ITEM_PAD: (f32, f32) = (10.0, 8.0);
const ITEM_GAP: f32 = 8.0;
/// `.dot { width: 8px; height: 8px }`.
const DOT: f32 = 8.0;
/// `.badge { padding: 1px 7px }`.
const BADGE_PAD: (f32, f32) = (7.0, 1.0);
/// `.palette-empty { padding: 22px 12px }`.
const EMPTY_PAD: f32 = 22.0;

/// The field's widget id for a mount.
pub fn input_id(mount: u64) -> Id {
    Id::new(PALETTE_ID).with(mount).with("input")
}

/// What the palette keeps between frames.
#[derive(Debug, Default)]
pub struct PaletteView {
    /// The query and the highlighted row.
    pub model: CommandPalette,
    /// `.palette-list`'s `scrollTop`.
    scroll: f32,
    /// The field took focus at mount.
    focused_once: bool,
    /// When the caret's blink restarted (focus, typing).
    blink_from: f64,
    /// The item under the pointer last frame: `mouseenter` fires on a change.
    hovered: Option<usize>,
}

impl PaletteView {
    pub fn new() -> Self {
        Self::default()
    }
}

pub struct PaletteProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// The window: the overlay is fixed to it.
    pub viewport: Rect,
    /// `commandRegistry.getCommands()`.
    pub commands: &'a [PaletteCommand],
    /// Each command's dot colour, parallel to `commands`.
    pub accents: &'a [Option<Rgba>],
    pub mount: u64,
    pub now: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteEvent {
    /// `onClose`.
    Close,
    /// A command ran: the palette closes, then its action runs.
    Run(PaletteAction),
}

struct Fonts {
    input: TextStyle,
    section: TextStyle,
    item: TextStyle,
    badge: TextStyle,
    hint: TextStyle,
    empty: TextStyle,
}

fn fonts(v: &Vars) -> Fonts {
    Fonts {
        input: TextStyle::new(14.0, 400)
            .line_height(LineHeight::Number(v.line_height))
            .nowrap(false),
        section: TextStyle::new(10.0, 600)
            .line_height(LineHeight::Number(v.line_height))
            .letter_spacing_em(0.08)
            .transform(TextTransform::Uppercase)
            .color(v.fg_subtle.to_color32()),
        item: TextStyle::new(13.0, 400)
            .line_height(LineHeight::Number(v.line_height))
            .nowrap(true)
            .color(v.fg.to_color32()),
        badge: TextStyle::new(10.0, 600)
            .line_height(LineHeight::Number(v.line_height))
            .nowrap(false)
            .color(v.fg_muted.to_color32()),
        hint: TextStyle::new(11.0, 400)
            .line_height(LineHeight::Number(v.line_height))
            .nowrap(false)
            .color(v.fg_subtle.to_color32()),
        empty: TextStyle::new(13.0, 400)
            .line_height(LineHeight::Number(v.line_height))
            .align(TextAlign::Center)
            .color(v.fg_muted.to_color32()),
    }
}

fn line(ctx: &egui::Context, style: &TextStyle) -> f32 {
    style.line_box(ctx).map_or(style.size * 1.25, |l| l.height)
}

/// A shaped width as layout holds it: ceiled to 1/64 device px.
fn width_of(ctx: &egui::Context, s: &str, style: &TextStyle, ppp: f32) -> f32 {
    let w = text::measure(ctx, s, style).unwrap_or(0.0);
    (w * ppp * 64.0).ceil() / 64.0 / ppp
}

/// `n * 100vh` as layout holds it: truncated to 1/64 device px (12vh of a
/// 440 px window is 52.796875).
fn vh(n: f32, vp: Rect, ppp: f32) -> f32 {
    (n * vp.height() * ppp * 64.0).floor() / 64.0 / ppp
}

/// A row of the list, placed in the list's content (from its padding box
/// top, before scrolling).
#[derive(Clone, Copy, Debug)]
struct Placed {
    row: Option<PaletteRow>,
    top: f32,
    height: f32,
}

/// Lays the rows out down the list; `None` is the empty state. Returns the
/// rows and the scrollable height (both paddings included).
fn place(
    ctx: &egui::Context,
    rows: &[PaletteRow],
    commands: &[PaletteCommand],
    f: &Fonts,
    border: f32,
) -> (Vec<Placed>, f32) {
    let mut y = LIST_PAD;
    let mut out = Vec::new();
    if rows.is_empty() {
        let h = 2.0 * EMPTY_PAD + line(ctx, &f.empty);
        out.push(Placed {
            row: None,
            top: y,
            height: h,
        });
        y += h;
    }
    let section_h = SECTION_PAD.1 + line(ctx, &f.section) + SECTION_PAD.2;
    let title = line(ctx, &f.item);
    let badge = line(ctx, &f.badge) + 2.0 * BADGE_PAD.1 + 2.0 * border;
    let hint = line(ctx, &f.hint);
    for row in rows {
        let height = match row {
            PaletteRow::Header(_) => section_h,
            PaletteRow::Command { command, .. } => {
                let c = &commands[*command];
                let mut cross = title;
                if c.accent.is_some() {
                    cross = cross.max(DOT);
                }
                if c.active {
                    cross = cross.max(badge);
                }
                if c.hint.is_some() {
                    cross = cross.max(hint);
                }
                cross + 2.0 * ITEM_PAD.1
            }
        };
        out.push(Placed {
            row: Some(*row),
            top: y,
            height,
        });
        y += height;
    }
    (out, y + LIST_PAD)
}

/// The field's keys: (moved the highlight, Enter pressed). Taken keys leave
/// the frame's input.
fn field_keys(ui: &Ui, view: &mut PaletteView, count: usize) -> (bool, bool) {
    let mut moved = false;
    let mut enter = false;
    let empty = view.model.query().is_empty();
    ui.ctx().input_mut(|i| {
        i.events.retain(|event| {
            let Event::Key {
                key, pressed: true, ..
            } = event
            else {
                return true;
            };
            match key {
                Key::ArrowDown => {
                    view.model.move_active(1, count);
                    moved = true;
                    false
                }
                Key::ArrowUp => {
                    view.model.move_active(-1, count);
                    moved = true;
                    false
                }
                Key::Home if empty => {
                    view.model.home();
                    moved = true;
                    false
                }
                Key::End if empty => {
                    view.model.end(count);
                    moved = true;
                    false
                }
                Key::Enter => {
                    enter = true;
                    false
                }
                _ => true,
            }
        })
    });
    (moved, enter)
}

/// Typing into the field: text appends, Backspace removes the last
/// character. Returns whether the query changed.
fn edit(ui: &Ui, view: &mut PaletteView) -> bool {
    let mut next = view.model.query().to_string();
    ui.input(|i| {
        for event in &i.events {
            match event {
                Event::Text(t) => next.push_str(t),
                Event::Key {
                    key: Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    next.pop();
                }
                _ => {}
            }
        }
    });
    if next == view.model.query() {
        return false;
    }
    view.model.set_query(next);
    true
}

pub fn command_palette(
    ui: &mut Ui,
    props: &PaletteProps,
    view: &mut PaletteView,
) -> Vec<PaletteEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let f = fonts(v);
    let id = Id::new(PALETTE_ID).with(props.mount);
    let input = input_id(props.mount);
    let mut events = Vec::new();

    if !view.focused_once {
        view.focused_once = true;
        ctx.memory_mut(|m| m.request_focus(input));
        view.blink_from = props.now;
    }
    // The field owns the arrows: egui's own focus moves are cancelled.
    ctx.memory_mut(|m| m.move_focus(FocusDirection::None));
    let focused = ctx.memory(|m| m.has_focus(input));
    if focused {
        crate::ui::components::focus::mark_text_field(&ctx, input);
        if edit(ui, view) {
            view.blink_from = props.now;
        }
    }
    let locale = props.locale;
    let flat = view.model.flat_commands(props.commands, locale);
    let (moved, enter) = if focused {
        field_keys(ui, view, flat.len())
    } else {
        (false, false)
    };
    if enter {
        if let Some(&command) = flat.get(view.model.active_index()) {
            let command = &props.commands[command];
            if !command.disabled {
                events.push(PaletteEvent::Run(command.action.clone()));
            }
        }
    }
    let rows = view.model.rows(props.commands, locale);

    // Geometry, in CSS px from the unsnapped box.
    let vp = props.viewport;
    let border = snap::border_width(v.border_px(), ppp);
    let rule = snap::border_width(1.0, ppp);
    let width = WIDTH.min(vp.width() - 48.0);
    let input_line = line(&ctx, &f.input);
    let input_h = 2.0 * INPUT_PAD.1 + input_line + rule;
    let (placed, content_h) = place(&ctx, &rows, props.commands, &f, rule);
    let max_list = vh(0.46, vp, ppp);
    let list_h = content_h.min(max_list);
    let height = 2.0 * border + input_h + list_h;
    let left = vp.left() + snap::centre(vp.width(), width, ppp);
    let top = vp.top() + vh(0.12, vp, ppp);
    let boxed = Rect::from_min_size(pos2(left, top), vec2(width, height));
    let inner = boxed.shrink(border);
    let input_rect = Rect::from_min_size(inner.min, vec2(inner.width(), input_h));
    let list = Rect::from_min_max(pos2(inner.left(), input_rect.bottom()), inner.max);
    let overflow = scroll::overflows(list.height(), content_h);
    let content_w = list.width() - if overflow { scroll::WIDTH } else { 0.0 };

    // Keyboard moves keep the highlighted item in view.
    if moved {
        let active = view.model.active_index();
        if let Some(p) = placed
            .iter()
            .find(|p| matches!(p.row, Some(PaletteRow::Command { index, .. }) if index == active))
        {
            // The scroll offset lands on a whole device pixel.
            let to = nearest(view.scroll, list.height(), p.top, p.top + p.height);
            view.scroll = (to * ppp).round() / ppp;
        }
    }

    let ramp = Keyframes::from_to(0.0f32, 1.0);
    let fade = anim::animate(&ctx, id.with("fade"), &ramp, &FADE, 1.0);
    let slide = anim::animate(&ctx, id.with("slide"), &ramp, &SLIDE, 1.0);

    // The overlay: a mousedown on it outside the box closes.
    let overlay = ui.interact(vp, id.with("overlay"), Sense::click());
    let pressed_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .press_origin()
                .is_some_and(|p| vp.contains(p) && !boxed.contains(p))
    });
    if pressed_outside && overlay.hovered() {
        events.push(PaletteEvent::Close);
    }
    let painter = ui.painter().clone();
    Mark::Begin {
        sigma: v.overlay_blur * ppp,
        opacity: fade,
        saturate: 1.0,
        radius: None,
        ppp,
    }
    .paint(&painter, vp);
    let mut wash = painter.clone();
    wash.multiply_opacity(fade);
    wash.rect_filled(vp, 0.0, v.bg.mix(0.45, Rgba::TRANSPARENT).to_color32());

    // `scale(.96) translateY(8px)` to none about the box centre.
    let scale = 0.96 + 0.04 * slide;
    let rise = 8.0 * (1.0 - slide);
    let centre = boxed.center().to_vec2();
    let transform = TSTransform::from_translation(centre + vec2(0.0, rise * scale))
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-centre);
    let mut run = None;
    ui.with_visual_transform(transform, |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(vp));
        child.multiply_opacity(slide);
        let snapped = snap::rect(boxed, ppp);
        child.interact(snapped, id.with("box"), Sense::click());
        BoxStyle {
            background: v.bg_card,
            images: v.card_bg_image.clone(),
            radius: Corners::all(10.0),
            border: Border::all(border, v.border),
            shadows: vec![BoxShadow {
                offset: vec2(0.0, 18.0),
                blur: 48.0,
                spread: 0.0,
                color: v.bg.mix(0.6, Rgba::TRANSPARENT),
                inset: false,
            }],
            ..BoxStyle::default()
        }
        .paint(child.painter(), snapped);

        paint_field(
            &mut child, props, view, &f, input, input_rect, rule, focused,
        );

        // The list: its scrollbar, then the rows clipped to its padding box.
        scroll::scroll(
            &mut child,
            list,
            id.with("list"),
            &mut view.scroll,
            content_h,
            v,
        );
        let clip = snap::rect(list, ppp);
        let painter = child.painter().with_clip_rect(clip);
        let pointer_in_list = child.rect_contains_pointer(clip);
        let mut hovered = None;
        for p in &placed {
            let y = list.top() + p.top - view.scroll;
            if y + p.height < list.top() || y > list.bottom() {
                continue;
            }
            let x0 = list.left() + LIST_PAD;
            let rect = Rect::from_min_size(pos2(x0, y), vec2(content_w - 2.0 * LIST_PAD, p.height));
            match p.row {
                None => {
                    let text_rect = Rect::from_min_max(
                        pos2(list.left() + 12.0 + LIST_PAD, y + EMPTY_PAD),
                        pos2(
                            list.left() + content_w - 12.0 - LIST_PAD,
                            y + p.height - EMPTY_PAD,
                        ),
                    );
                    let label = i18n::translate(locale, "palette.noResults");
                    text::paint(&painter, text_rect, &label, &f.empty);
                }
                Some(PaletteRow::Header(section)) => {
                    paint_header(&painter, &f, rect, section, locale);
                }
                Some(PaletteRow::Command { index, command }) => {
                    let response = child.interact(
                        rect.intersect(clip),
                        id.with(("item", command)),
                        Sense::click(),
                    );
                    if pointer_in_list && response.hovered() {
                        hovered = Some(index);
                    }
                    let c = &props.commands[command];
                    if response.clicked() && !c.disabled {
                        run = Some(c.action.clone());
                    }
                    let accent = props.accents.get(command).copied().flatten();
                    let active = index == view.model.active_index();
                    paint_item(&painter, &f, rect, c, accent, active, locale, rule, v);
                }
            }
        }
        // `mouseenter`: entering an item highlights it.
        if hovered != view.hovered {
            if let Some(index) = hovered {
                let count = flat.len();
                if index < count {
                    let delta = index as i64 - view.model.active_index() as i64;
                    view.model.move_active(delta, count);
                }
            }
            view.hovered = hovered;
        }
    });
    Mark::End.paint(&painter, vp);
    if let Some(action) = run {
        events.push(PaletteEvent::Run(action));
    }
    events
}

#[allow(clippy::too_many_arguments)]
fn paint_field(
    ui: &mut Ui,
    props: &PaletteProps,
    view: &PaletteView,
    f: &Fonts,
    id: Id,
    rect: Rect,
    rule: f32,
    focused: bool,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let field = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - rule));
    let response = ui
        .interact(snap::rect(field, ppp), id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::Text);
    let placeholder = i18n::translate(props.locale, "palette.placeholder");
    response.widget_info(|| {
        egui::WidgetInfo::text_edit(true, view.model.query(), view.model.query(), &placeholder)
    });
    if response.clicked() {
        response.request_focus();
    }
    // `border-bottom: 1px solid var(--border)`.
    let bottom = Rect::from_min_max(pos2(rect.left(), rect.bottom() - rule), rect.max);
    ui.painter()
        .rect_filled(snap::rect(bottom, ppp), 0.0, v.border.to_color32());
    let line = line(&ctx, &f.input);
    let text_rect = Rect::from_min_size(
        pos2(rect.left() + INPUT_PAD.0, rect.top() + INPUT_PAD.1),
        vec2(rect.width() - 2.0 * INPUT_PAD.0, line),
    );
    let painter = ui
        .painter()
        .with_clip_rect(ui.clip_rect().intersect(Rect::from_min_max(
            pos2(text_rect.left(), field.top()),
            pos2(text_rect.right(), field.bottom()),
        )));
    let query = view.model.query();
    if query.is_empty() {
        let style = f.input.clone().color(v.fg_subtle.to_color32());
        text::paint(&painter, text_rect, &placeholder, &style);
    } else {
        let style = f.input.clone().color(v.fg.to_color32());
        text::paint(&painter, text_rect, query, &style);
    }
    if focused {
        let now = props.now;
        let phase = ((now - view.blink_from) / CARET_BLINK).floor() as i64;
        if phase % 2 == 0 {
            let text_w = text::measure(&ctx, query, &f.input).unwrap_or(0.0);
            let baseline = f.input.line_box(&ctx).map_or(line, |l| l.baseline);
            let caret = caret_rect(text_rect, baseline, text_w, !query.is_empty(), ppp);
            ui.painter().rect_filled(caret, 0.0, v.fg.to_color32());
        }
        let next_flip = view.blink_from + (phase + 1) as f64 * CARET_BLINK;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            (next_flip - now).max(0.0),
        ));
    }
}

/// Blink's caret in the 14 px field: from the baseline up the ascent and
/// down the descent of Segoe UI's win metrics (2210 and 514 per 2048), each
/// rounded in device px (measured on WebView2: 19 px at 1x, 28 at 1.5x;
/// the 13 px dialog field gives 17 and 26). Its left edge is the text's
/// end in layout units, moved back half the caret's width and rounded
/// after text, floored on an empty field; it is 1 CSS px wide.
fn caret_rect(line: Rect, baseline: f32, text_w: f32, after_text: bool, ppp: f32) -> Rect {
    let size = 14.0 * ppp;
    let ascent = (size * 2210.0 / 2048.0).round();
    let descent = (size * 514.0 / 2048.0).round();
    let base = snap::round_device((line.top() + baseline) * ppp);
    let caret = ppp.max(1.0);
    let end = snap::layout_unit(line.left() * ppp) + (text_w * ppp * 64.0).floor() / 64.0;
    let left = if after_text {
        (end - caret / 2.0 + 0.5).floor()
    } else {
        end.floor()
    };
    let w = ((left + caret + 0.5).floor() - left).max(1.0);
    Rect::from_min_size(
        pos2(left, base - ascent) / ppp,
        vec2(w, ascent + descent) / ppp,
    )
}

fn paint_header(painter: &Painter, f: &Fonts, rect: Rect, section: PaletteSection, locale: Locale) {
    let label = i18n::translate(locale, section.label_key());
    let r = Rect::from_min_max(
        pos2(rect.left() + SECTION_PAD.0, rect.top() + SECTION_PAD.1),
        pos2(rect.right() - SECTION_PAD.0, rect.bottom() - SECTION_PAD.2),
    );
    text::paint(painter, r, &label, &f.section);
}

#[allow(clippy::too_many_arguments)]
fn paint_item(
    painter: &Painter,
    f: &Fonts,
    rect: Rect,
    command: &PaletteCommand,
    accent: Option<Rgba>,
    active: bool,
    locale: Locale,
    rule: f32,
    v: &Vars,
) {
    let ctx = painter.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let mut painter = painter.clone();
    if command.disabled {
        painter.multiply_opacity(0.45);
    }
    if active {
        BoxStyle {
            background: v.fg.mix(0.09, Rgba::TRANSPARENT),
            radius: Corners::all(6.0),
            ..BoxStyle::default()
        }
        .paint(&painter, snap::rect(rect, ppp));
    }
    let content = Rect::from_min_max(
        pos2(rect.left() + ITEM_PAD.0, rect.top() + ITEM_PAD.1),
        pos2(rect.right() - ITEM_PAD.0, rect.bottom() - ITEM_PAD.1),
    );
    let cross = content.height();
    let at = |h: f32| content.top() + snap::centre(cross, h, ppp);
    let mut x = content.left();
    let mut right = content.right();
    if let Some(color) = accent {
        let dot = Rect::from_min_size(pos2(x, at(DOT)), vec2(DOT, DOT));
        BoxStyle {
            background: color,
            radius: Corners::all(DOT / 2.0),
            ..BoxStyle::default()
        }
        .paint(&painter, snap::rect(dot, ppp));
        x += DOT + ITEM_GAP;
    }
    if let Some(hint) = &command.hint {
        let w = width_of(&ctx, hint, &f.hint, ppp);
        let h = line(&ctx, &f.hint);
        let r = Rect::from_min_size(pos2(right - w, at(h)), vec2(w, h));
        text::paint(&painter, r, hint, &f.hint);
        right -= w + ITEM_GAP;
    }
    if command.active {
        let label = i18n::translate(locale, "common.active");
        let tw = width_of(&ctx, &label, &f.badge, ppp);
        let lh = line(&ctx, &f.badge);
        let w = tw + 2.0 * BADGE_PAD.0 + 2.0 * rule;
        let h = lh + 2.0 * BADGE_PAD.1 + 2.0 * rule;
        let r = Rect::from_min_size(pos2(right - w, at(h)), vec2(w, h));
        BoxStyle {
            radius: Corners::all(999.0),
            border: Border::all(rule, v.border),
            ..BoxStyle::default()
        }
        .paint(&painter, snap::rect(r, ppp));
        let t = Rect::from_min_size(
            pos2(r.left() + rule + BADGE_PAD.0, r.top() + rule + BADGE_PAD.1),
            vec2(tw, lh),
        );
        text::paint(&painter, t, &label, &f.badge);
        right -= w + ITEM_GAP;
    }
    let h = line(&ctx, &f.item);
    let title = command.title.resolve(locale);
    let r = Rect::from_min_max(pos2(x, at(h)), pos2(right.max(x), at(h) + h));
    text::paint(&painter, r, &title, &f.item);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_scrolls_the_least() {
        // Above the scrollport: its top aligns.
        assert_eq!(nearest(100.0, 200.0, 40.0, 72.0), 40.0);
        // Below: its bottom aligns.
        assert_eq!(nearest(0.0, 200.0, 190.0, 222.0), 22.0);
        // Inside: no scroll.
        assert_eq!(nearest(10.0, 200.0, 40.0, 72.0), 10.0);
    }
}
