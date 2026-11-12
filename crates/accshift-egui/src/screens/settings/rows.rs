//! Painting one item of a card into the rect its height was measured for.
//! Each returns what the user did with it.

use egui::{pos2, vec2, Id, Rect, Ui};

use super::{
    field_label_style, hint_style, measure, platform_tab, platforms_tab, steam_tab, theme_grid,
    Control, Ctx, InputField, Item,
};
use crate::app::settings_panel::{js_number, SettingsAction};
use crate::app::vars::Vars;
use crate::ui::components::controls::button::{self, ButtonProps, ButtonStyle};
use crate::ui::components::controls::select::{self, SelectProps};
use crate::ui::components::controls::slider::{self, SliderProps};
use crate::ui::components::controls::text_input::{self, TextInputProps};
use crate::ui::components::controls::{card, label_target, toggle};
use crate::ui::snap;
use crate::ui::text::{self, TextStyle};

/// The top of a field's control: the 18 px label and the 8 px gap.
const CONTROL_TOP: f32 = 26.0;

pub(super) fn paint_item(
    ui: &mut Ui,
    r: Rect,
    id: Id,
    c: &Ctx,
    item: &Item,
) -> Vec<SettingsAction> {
    let v = &c.view.vars;
    let mut out = Vec::new();
    match item {
        Item::Title(text) => {
            card::paint_title(ui, r, text, v);
        }
        Item::TitleRow(text, help) => {
            if title_row(ui, r, id, c, text, help) {
                out.push(SettingsAction::OpenIntegrationsWiki);
            }
        }
        Item::Field {
            label,
            hint,
            control,
        } => out.extend(field(ui, r, id, c, label, hint.as_deref(), control)),
        Item::Toggle {
            label,
            description,
            enabled,
            disabled,
            action,
        } => {
            let on_off = c.on_off();
            let props = c.toggle_props(label, description, *enabled, *disabled, &on_off);
            if toggle::toggle(ui, r, id.with("toggle"), &props) {
                out.push(action.clone());
            }
        }
        Item::ThemeGrid => out.extend(theme_grid::paint(ui, r, id, c)),
        Item::Button {
            label,
            enabled,
            action,
        } => {
            let style = button::export_style(v);
            if fitted_button(ui, r, id.with("button"), c, label, &style, *enabled, None) {
                out.push(action.clone());
            }
        }
        Item::Link(label, action) => {
            let style = button::link_style(v);
            if fitted_button(ui, r, id.with("link"), c, label, &style, true, Some(18.0)) {
                out.push(action.clone());
            }
        }
        Item::Hint(text, danger) | Item::FieldHint(text, danger) => {
            let mut style = hint_style(v);
            if *danger {
                style = style.color(v.danger.to_color32());
            }
            text::paint(
                ui.painter(),
                Rect::from_min_size(r.min, vec2(r.width(), 0.0)),
                text,
                &style,
            );
        }
        Item::Stack(gap, items) => {
            let mut y = r.top();
            for (i, child) in items.iter().enumerate() {
                let h = measure::item_height(ui.ctx(), c, child, r.width());
                let rect = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), h));
                out.extend(paint_item(ui, rect, id.with(i), c, child));
                y += h + gap;
            }
        }
        Item::Platforms(item) => out.extend(platforms_tab::paint(ui, r, id, c, item)),
        Item::Platform(item) => out.extend(platform_tab::paint(ui, r, id, c, item)),
        Item::Steam(item) => out.extend(steam_tab::paint(ui, r, id, c, item)),
    }
    out
}

/// A divider above, the title and a "?" button on the right. True when
/// the button was activated.
fn title_row(ui: &mut Ui, r: Rect, id: Id, c: &Ctx, title: &str, help: &str) -> bool {
    let v = &c.view.vars;
    let ppp = ui.ctx().pixels_per_point();
    card::paint_divider(ui, r, v);
    let h3_top = r.top() + snap::centre(20.0, 18.0, ppp);
    text::paint(
        ui.painter(),
        Rect::from_min_size(pos2(r.left(), h3_top), vec2(r.width() - 28.0, 18.0)),
        title,
        &card::title_style(v),
    );
    let wiki = Rect::from_min_size(pos2(r.right() - 20.0, r.top()), vec2(20.0, 20.0));
    let style = button::wiki_style(v);
    let props = ButtonProps {
        label: "?",
        aria_label: Some(help),
        style: &style,
        enabled: true,
        vars: v,
        focus: c.focus,
    };
    button::button(ui, wiki, id.with("wiki"), &props)
}

/// A button as wide as its label (never wider than the row) at the row's
/// top left, `height` tall or as tall as its style makes it.
#[allow(clippy::too_many_arguments)]
fn fitted_button(
    ui: &mut Ui,
    r: Rect,
    id: Id,
    c: &Ctx,
    label: &str,
    style: &ButtonStyle,
    enabled: bool,
    height: Option<f32>,
) -> bool {
    let size = button::size(ui.ctx(), label, style);
    let rect = Rect::from_min_size(r.min, vec2(size.x.min(r.width()), height.unwrap_or(size.y)));
    let props = ButtonProps {
        label,
        aria_label: None,
        style,
        enabled,
        vars: &c.view.vars,
        focus: c.focus,
    };
    button::button(ui, rect, id, &props)
}

/// The label (a click on it focuses the control), its inline hint, then
/// the control.
fn field(
    ui: &mut Ui,
    r: Rect,
    id: Id,
    c: &Ctx,
    label: &str,
    hint: Option<&str>,
    control: &Control,
) -> Vec<SettingsAction> {
    let v = &c.view.vars;
    let control_id = id.with(control.salt());
    label_target(ui, r, id.with("label"), control_id);
    let label_rect = Rect::from_min_size(r.min, vec2(r.width(), 18.0));
    let style = field_label_style(v);
    text::paint(ui.painter(), label_rect, label, &style);
    if let Some(hint) = hint {
        paint_label_hint(ui, r, v, label, hint, &style);
    }
    let control_top = r.top() + CONTROL_TOP;
    let mut out = Vec::new();
    match control {
        Control::Select {
            options,
            selected,
            actions,
            disabled,
        } => {
            let height = text_input::height(ui.ctx().pixels_per_point());
            let rect = Rect::from_min_size(pos2(r.left(), control_top), vec2(r.width(), height));
            let props = SelectProps {
                options,
                selected: *selected,
                disabled,
                label,
                vars: v,
                look: None,
            };
            let ev = select::select(ui, rect, control_id, &props);
            if let Some(action) = ev.changed.and_then(|i| actions.get(i)) {
                out.push(action.clone());
            }
        }
        Control::Slider {
            min,
            max,
            step,
            field,
            commit_on_input,
        } => {
            let rect = Rect::from_min_size(pos2(r.left(), control_top), vec2(r.width(), 16.0));
            let value = js_number(&c.panel.numeric(*field).input).unwrap_or(*min);
            let props = SliderProps {
                min: *min,
                max: *max,
                step: *step,
                value,
                label,
                dark: c.view.dark,
                vars: v,
            };
            let ev = slider::slider(ui, rect, control_id, &props);
            let mut set = |value: f64, commit: bool| {
                out.push(SettingsAction::NumericInput(*field, format_number(value)));
                if commit {
                    out.push(SettingsAction::NumericCommit(*field));
                }
            };
            // Every move updates the field; it commits on the move itself
            // or on release.
            if let Some(value) = ev.input {
                set(value, *commit_on_input);
            }
            if let Some(value) = ev.change.filter(|_| !commit_on_input) {
                set(value, true);
            }
        }
        Control::Input {
            kind,
            field,
            placeholder,
            enabled,
        } => {
            let height = text_input::height(ui.ctx().pixels_per_point());
            let rect = Rect::from_min_size(pos2(r.left(), control_top), vec2(r.width(), height));
            let mut value = field.value(c.panel);
            let style = text_input::settings_style(v);
            let props = TextInputProps {
                style: &style,
                kind: *kind,
                placeholder,
                label,
                max_len: field.max_len(),
                enabled: *enabled,
                vars: v,
                dark: c.view.dark,
            };
            let ev = text_input::text_input(ui, rect, control_id, &mut value, &props);
            if ev.changed {
                out.push(field.input_action(value));
            }
            // A number commits when Enter or a blur ends the edit; Enter
            // also gives the focus up.
            if let InputField::Numeric(f) = field {
                if ev.enter || ev.blurred {
                    out.push(SettingsAction::NumericCommit(*f));
                }
                if ev.enter {
                    ui.memory_mut(|m| m.surrender_focus(control_id));
                }
            }
        }
    }
    out
}

/// A hint inline after the label, on the label's baseline.
fn paint_label_hint(ui: &Ui, r: Rect, v: &Vars, label: &str, hint: &str, label_style: &TextStyle) {
    let ctx = ui.ctx();
    let w = text::measure(ctx, &format!("{label} "), label_style).unwrap_or(0.0);
    let hs = hint_style(v).nowrap(false);
    let base = label_style.line_box(ctx).map_or(0.0, |b| b.baseline);
    let hb = hs.line_box(ctx).map_or(0.0, |b| b.baseline);
    text::paint(
        ui.painter(),
        Rect::from_min_size(
            pos2(r.left() + w, r.top() + base - hb),
            vec2(r.width() - w, 16.0),
        ),
        hint,
        &hs,
    );
}

/// A slider value printed as a number field prints it: no fraction when
/// it is whole.
fn format_number(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
