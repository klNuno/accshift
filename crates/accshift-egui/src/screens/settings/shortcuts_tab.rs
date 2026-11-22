//! The Shortcuts tab: the two app actions, then the accounts and personas
//! with a binding and the picker that adds one.
//!
//! ```text
//! card     title, an "i" when closing the app stops the shortcuts
//!   row    the name (and the platform) as a field label, the combination
//!          box under it with its clear button, a line under it for a
//!          conflict
//! ```
//!
//! The combination box listens on a click ([`SettingsAction::ShortcutListen`]):
//! the app reads the next press, Escape cancels.

use accshift_app::i18n::{translate_with, Locale, TranslationValue};
use accshift_app::shortcuts::{RowStatus, ShortcutTarget};
use egui::{pos2, vec2, Context, Id, LayerId, Order, Rect, Ui};

use super::{field_label_style, hint_style, measure, tr, CardSpec, Control, Ctx, FIELD_GAP};
use crate::app::settings_panel::SettingsAction;
use crate::app::shortcuts::{DuplicateOf, TAB};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::components::controls::button::{self, ButtonProps, ButtonStyle};
use crate::ui::components::controls::{card, text_input};
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// The label line above the box, as a field's.
const LABEL_H: f32 = 18.0;
/// The clear button, square, and its gap to the box.
const CLEAR_W: f32 = 20.0;
const CLEAR_GAP: f32 = 6.0;
/// Between the row and its conflict line.
const NOTE_GAP: f32 = 4.0;
/// The "i" and its tooltip, which fades in after 300 ms.
const INFO_W: f32 = 18.0;
const TIP_W: f32 = 240.0;
const TIP_IN: Transition = Transition::new(0.12, Timing::EASE_OUT).with_delay(0.3);
const TIP_OUT: Transition = Transition::new(0.12, Timing::EASE_OUT);

/// What the Shortcuts tab reads, built by the app each frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ShortcutsView {
    /// The session takes global shortcuts (a Wayland session needs the
    /// portal).
    pub available: bool,
    /// Closing the window quits, so the shortcuts stop with it.
    pub stops_on_close: bool,
    pub rows: Vec<ShortcutRow>,
    /// The listening row's last press was not usable.
    pub unusable: bool,
    /// The accounts and personas without a binding, label first.
    pub picker: Vec<(String, ShortcutTarget)>,
}

impl Default for ShortcutsView {
    fn default() -> Self {
        Self {
            available: true,
            stops_on_close: false,
            rows: Vec::new(),
            unusable: false,
            picker: Vec::new(),
        }
    }
}

/// One binding row.
#[derive(Debug, Clone, PartialEq)]
pub struct ShortcutRow {
    pub target: ShortcutTarget,
    /// The account's or persona's name; `None` for the two app actions.
    pub name: Option<String>,
    /// The account's platform.
    pub platform: Option<String>,
    /// The combination as this OS writes it, `None` when unset.
    pub combo: Option<String>,
    /// The trigger the system bound instead, when it differs (the portal).
    pub trigger: Option<String>,
    pub status: Option<RowStatus>,
    pub duplicate: Option<DuplicateOf>,
    pub listening: bool,
}

/// One of the tab's own items.
pub(super) enum Item {
    /// The card title, with the "i" when `info` is set.
    Heading { title: String, info: Option<String> },
    /// A row of [`ShortcutsView::rows`], by index.
    Row(usize),
}

fn is_action(target: &ShortcutTarget) -> bool {
    matches!(
        target,
        ShortcutTarget::OpenPalette | ShortcutTarget::ToggleWindow
    )
}

fn action_label(l: Locale, target: &ShortcutTarget) -> String {
    match target {
        ShortcutTarget::OpenPalette => tr(l, "settings.shortcutsOpenPalette"),
        _ => tr(l, "settings.shortcutsToggleWindow"),
    }
}

pub(super) fn cards(c: &Ctx) -> Vec<CardSpec> {
    let l = c.locale;
    let t = |k: &str| tr(l, k);
    let view = &c.view.shortcuts;
    let mut first = vec![super::Item::Shortcuts(Item::Heading {
        title: t("settings.shortcutsTitle"),
        info: view.stops_on_close.then(|| t("settings.shortcutsInfo")),
    })];
    if !view.available {
        first.push(super::Item::Hint(t("settings.shortcutsUnavailable"), false));
    }
    let mut second = vec![super::Item::Title(t("settings.shortcutsItems"))];
    for (i, row) in view.rows.iter().enumerate() {
        let item = super::Item::Shortcuts(Item::Row(i));
        if is_action(&row.target) {
            first.push(item);
        } else {
            second.push(item);
        }
    }
    if !view.picker.is_empty() {
        let mut options = vec![t("settings.shortcutsAddPlaceholder")];
        // The placeholder is disabled: its action never runs.
        let mut actions = vec![SettingsAction::SelectTab(TAB.to_string())];
        let mut disabled = vec![true];
        for (label, target) in &view.picker {
            options.push(label.clone());
            actions.push(SettingsAction::ShortcutListen(target.clone()));
            disabled.push(false);
        }
        second.push(super::Item::Field {
            label: t("settings.shortcutsAdd"),
            hint: None,
            control: Control::Select {
                options,
                selected: 0,
                actions,
                disabled,
            },
        });
    }
    vec![
        CardSpec {
            wide: true,
            items: first,
        },
        CardSpec {
            wide: true,
            items: second,
        },
    ]
}

/// The line under a row: what went wrong, or what the listening row wants.
fn note(c: &Ctx, row: &ShortcutRow) -> Option<(String, bool)> {
    let l = c.locale;
    if row.listening {
        let key = if c.view.shortcuts.unusable {
            "settings.shortcutsUnusable"
        } else {
            "settings.shortcutsCancelHint"
        };
        return Some((tr(l, key), false));
    }
    let text = match row.status.as_ref()? {
        RowStatus::Taken => tr(l, "settings.shortcutsTaken"),
        RowStatus::Failed => tr(l, "settings.shortcutsFailed"),
        RowStatus::Duplicate(_) => {
            let name = match row.duplicate.as_ref()? {
                DuplicateOf::Action(target) => action_label(l, target),
                DuplicateOf::Named(name) => name.clone(),
            };
            translate_with(
                l,
                "settings.shortcutsDuplicate",
                &[("name", TranslationValue::from(name))],
            )
        }
        RowStatus::Pending => return Some((tr(l, "settings.shortcutsPending"), false)),
        RowStatus::Active => {
            let trigger = row.trigger.as_ref()?;
            let text = translate_with(
                l,
                "settings.shortcutsBoundTo",
                &[("trigger", TranslationValue::from(trigger.clone()))],
            );
            return Some((text, false));
        }
        RowStatus::Unavailable | RowStatus::Paused => return None,
    };
    Some((text, true))
}

pub(super) fn height(ctx: &Context, c: &Ctx, item: &Item, width: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    match item {
        Item::Heading { .. } => card::title_height(ppp),
        Item::Row(i) => {
            let Some(row) = c.view.shortcuts.rows.get(*i) else {
                return 0.0;
            };
            let base = LABEL_H + FIELD_GAP + text_input::height(ppp);
            match note(c, row) {
                Some((text, _)) => {
                    base + NOTE_GAP
                        + measure::wrapped_height(ctx, &text, &hint_style(&c.view.vars), width)
                }
                None => base,
            }
        }
    }
}

pub(super) fn paint(ui: &mut Ui, r: Rect, id: Id, c: &Ctx, item: &Item) -> Vec<SettingsAction> {
    match item {
        Item::Heading { title, info } => {
            heading(ui, r, id, c, title, info.as_deref());
            Vec::new()
        }
        Item::Row(i) => c
            .view
            .shortcuts
            .rows
            .get(*i)
            .map(|row| paint_row(ui, r, id, c, row))
            .unwrap_or_default(),
    }
}

fn detail_style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 400)
        .line_height(LineHeight::Px(LABEL_H))
        .color(v.fg_subtle.to_color32())
        .nowrap(true)
}

/// The combination box: the input's look, its border in the accent while
/// it listens.
fn box_style(v: &Vars, listening: bool, set: bool) -> ButtonStyle {
    let border = if listening { v.accent } else { v.border };
    ButtonStyle {
        font: TextStyle::new(12.0, if set { 600 } else { 400 }).line_height(LineHeight::Px(18.0)),
        padding: vec2(10.0, 0.0),
        border: 1.0,
        radius: 8.0,
        background: v.bg_input,
        background_hover: v.bg_input,
        border_color: border,
        border_hover: if listening {
            v.accent
        } else {
            v.fg.mix(0.35, v.border)
        },
        color: if set && !listening { v.fg } else { v.fg_subtle },
        color_hover: if set && !listening { v.fg } else { v.fg_muted },
        underline_hover: false,
        disabled_opacity: 0.5,
        transitions: button::Transitions {
            background: false,
            border: true,
            color: true,
        },
    }
}

fn paint_row(ui: &mut Ui, r: Rect, id: Id, c: &Ctx, row: &ShortcutRow) -> Vec<SettingsAction> {
    let v = &c.view.vars;
    let l = c.locale;
    let ppp = ui.ctx().pixels_per_point();
    let height = text_input::height(ppp);
    let mut out = Vec::new();

    // The label line: the name, then the platform or "Persona".
    let name = row
        .name
        .clone()
        .unwrap_or_else(|| action_label(l, &row.target));
    let label_style = field_label_style(v).nowrap(true);
    let detail = match &row.target {
        ShortcutTarget::Account { .. } => row.platform.clone(),
        ShortcutTarget::Persona { .. } => Some(tr(l, "settings.shortcutsPersona")),
        _ => None,
    }
    .map(|d| format!(" \u{b7} {d}"));
    let detail_w = detail.as_ref().map_or(0.0, |d| {
        text::measure(ui.ctx(), d, &detail_style(v)).unwrap_or(0.0)
    });
    let name_w = (r.width() - detail_w).max(0.0);
    text::paint(
        ui.painter(),
        Rect::from_min_size(r.min, vec2(name_w, LABEL_H)),
        &name,
        &label_style,
    );
    if let Some(detail) = detail {
        let shown = text::measure(ui.ctx(), &name, &label_style)
            .unwrap_or(name_w)
            .min(name_w);
        text::paint(
            ui.painter(),
            Rect::from_min_size(pos2(r.left() + shown, r.top()), vec2(detail_w, LABEL_H)),
            &detail,
            &detail_style(v),
        );
    }

    // The box and its clear button, whose room stays kept so the boxes of
    // every row line up.
    let line = Rect::from_min_size(
        pos2(r.left(), r.top() + LABEL_H + FIELD_GAP),
        vec2(r.width(), height),
    );
    let clear = Rect::from_min_size(
        pos2(
            line.right() - CLEAR_W,
            line.top() + snap::centre(height, CLEAR_W, ppp),
        ),
        vec2(CLEAR_W, CLEAR_W),
    );
    let combo_box = Rect::from_min_max(line.min, pos2(clear.left() - CLEAR_GAP, line.bottom()));
    let label = if row.listening {
        tr(l, "settings.shortcutsListening")
    } else {
        row.combo
            .clone()
            .unwrap_or_else(|| tr(l, "settings.shortcutsNone"))
    };
    let style = box_style(v, row.listening, row.combo.is_some());
    let aria = format!("{name}: {label}");
    let props = ButtonProps {
        label: &label,
        aria_label: Some(&aria),
        style: &style,
        enabled: c.view.shortcuts.available,
        vars: v,
        focus: c.focus,
    };
    if button::button(ui, combo_box, id.with("combo"), &props) {
        out.push(SettingsAction::ShortcutListen(row.target.clone()));
    }
    if row.combo.is_some() || row.listening {
        let clear_style = button::wiki_style(v);
        let clear_label = tr(l, "settings.shortcutsClear");
        let props = ButtonProps {
            label: "\u{d7}",
            aria_label: Some(&clear_label),
            style: &clear_style,
            enabled: true,
            vars: v,
            focus: c.focus,
        };
        if button::button(ui, clear, id.with("clear"), &props) {
            out.push(SettingsAction::ShortcutClear(row.target.clone()));
        }
    }

    if let Some((text, danger)) = note(c, row) {
        let mut style = hint_style(v);
        if danger {
            style = style.color(v.danger.to_color32());
        }
        text::paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(line.left(), line.bottom() + NOTE_GAP),
                vec2(r.width(), 0.0),
            ),
            &text,
            &style,
        );
    }
    out
}

/// The title, and on its right the "i" whose tooltip says the shortcuts
/// stop with the app.
fn heading(ui: &mut Ui, r: Rect, id: Id, c: &Ctx, title: &str, info: Option<&str>) {
    let v = &c.view.vars;
    card::paint_title(ui, r, title, v);
    let Some(info) = info else {
        return;
    };
    let ppp = ui.ctx().pixels_per_point();
    let rect = Rect::from_min_size(
        pos2(
            r.right() - INFO_W,
            r.top() + snap::centre(18.0, INFO_W, ppp),
        ),
        vec2(INFO_W, INFO_W),
    );
    let style = button::wiki_style(v);
    let props = ButtonProps {
        label: "i",
        aria_label: Some(info),
        style: &style,
        enabled: true,
        vars: v,
        focus: c.focus,
    };
    let button_id = id.with("info");
    button::button(ui, rect, button_id, &props);
    let shown = ui.rect_contains_pointer(rect) || ui.memory(|m| m.has_focus(button_id));
    tooltip(ui, rect, id.with("tip"), info, shown, v);
}

/// A small box under `anchor`, right-aligned to it, wrapped at 240 px.
fn tooltip(ui: &Ui, anchor: Rect, id: Id, text: &str, shown: bool, v: &Vars) {
    let ctx = ui.ctx();
    let spec = if shown { TIP_IN } else { TIP_OUT };
    let opacity = anim::transition(ctx, id, if shown { 1.0f32 } else { 0.0 }, &spec);
    if opacity <= 0.0 {
        return;
    }
    let style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.4))
        .color(v.fg.to_color32());
    let border = v.border_px();
    let one_line = text::measure(ctx, text, &style).unwrap_or(TIP_W);
    let text_w = one_line.min(TIP_W);
    let text_h = measure::wrapped_height(ctx, text, &style, text_w);
    let size = vec2(text_w + 16.0 + 2.0 * border, text_h + 10.0 + 2.0 * border);
    let rect = Rect::from_min_size(pos2(anchor.right() - size.x, anchor.bottom() + 6.0), size);
    let mut painter = ctx.layer_painter(LayerId::new(Order::Foreground, id));
    painter.multiply_opacity(opacity);
    BoxStyle {
        background: v.bg_overlay,
        radius: Corners::all(6.0),
        border: Border::all(border, v.border),
        shadows: vec![BoxShadow {
            offset: vec2(0.0, 4.0),
            blur: 12.0,
            spread: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.3),
            inset: false,
        }],
        ..BoxStyle::default()
    }
    .paint(&painter, rect);
    text::paint(
        &painter,
        Rect::from_min_size(
            pos2(rect.left() + border + 8.0, rect.top() + border + 5.0),
            vec2(text_w, 0.0),
        ),
        text,
        &style,
    );
}
