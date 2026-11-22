use accshift_theme::{ColorScheme, TokenKind};

use super::super::*;
use super::controls::{checkbox, color_input};
use super::css_field::css_field;
use super::layout::{label_width, line_h, meta_tracks, pill_size, Block, Model};
use super::{
    faded_button, reset_style, Styles, BETA_GAP, CHECK, CHECK_GAP, COLOR_H, COLOR_W, CONTROL_GAP,
    FIELD_GAP, LABEL_GAP, META_GAP, RESET, ROW_GAP, ROW_PAD,
};
use crate::app::theme_editor::{self as model, ThemeEditor};
use crate::ui::components::controls::button as panel_button;
use crate::ui::components::controls::select::{self, SelectProps};
use crate::ui::components::controls::text_input::settings_style;

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_block(
    ui: &mut Ui,
    r: Rect,
    id: Id,
    block: &Block,
    s: &Styles,
    m: &Model,
    editor: &mut ThemeEditor,
    props: &DialogProps,
    field_h: f32,
    width: f32,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let locale = props.locale;
    let field = settings_style(v);
    match block {
        Block::Beta => {
            let label = i18n::translate(locale, "themeEditor.beta");
            let pill = pill_size(&ctx, s, &label);
            let pill_rect = snap::rect(
                Rect::from_min_size(
                    pos2(r.left(), r.top() + snap::centre(r.height(), pill.y, ppp)),
                    pill,
                ),
                ppp,
            );
            BoxStyle {
                radius: Corners::all(v.radius_sm),
                border: Border::all(snap::border_width(1.0, ppp), v.border),
                ..BoxStyle::default()
            }
            .paint(ui.painter(), pill_rect);
            let b = snap::border_width(1.0, ppp);
            text::paint(
                ui.painter(),
                Rect::from_min_size(
                    pill_rect.min + vec2(b + 6.0, b + 1.0),
                    vec2(pill.x - 14.0, line_h(&ctx, &s.pill)),
                ),
                &label,
                &s.pill,
            );
            let note = i18n::translate(locale, "themeEditor.betaNote");
            let x = r.left() + pill.x + BETA_GAP;
            let h = text_height(&ctx, &note, &s.beta, r.right() - x);
            text::paint(
                ui.painter(),
                Rect::from_min_max(
                    pos2(x, r.top() + snap::centre(r.height(), h, ppp)),
                    pos2(r.right(), r.bottom()),
                ),
                &note,
                &s.beta,
            );
        }
        Block::Meta => meta_grid(ui, r, id, s, m, editor, props, field_h),
        Block::Head(title) => {
            text::paint(ui.painter(), r, title, &s.h4);
        }
        Block::Token(i) => {
            let row = &m.rows[*i];
            let label_w = label_width(width);
            let line = Rect::from_min_size(pos2(r.left(), r.top() + ROW_PAD), vec2(width, field_h));
            // The label, centred on the row.
            let style = if row.overridden {
                &s.token_on
            } else {
                &s.token
            };
            let lh = line_h(&ctx, style);
            let ly = line.top() + snap::centre(field_h, lh, ppp);
            let text_w = text::paint(
                ui.painter(),
                Rect::from_min_size(pos2(line.left(), ly), vec2(label_w, lh)),
                &row.label,
                style,
            )
            .map_or(0.0, |l| l.width);
            if !row.overridden {
                let label = i18n::translate(locale, "themeEditor.inherited");
                let tw = snap::content_width(
                    text::measure(&ctx, &label, &s.inherited).unwrap_or(0.0),
                    ppp,
                );
                let ih = line_h(&ctx, &s.inherited);
                let size = vec2(tw + 8.0, ih + 2.0);
                let at = pos2(
                    line.left() + snap::content_width(text_w, ppp) + LABEL_GAP,
                    line.top() + snap::centre(field_h, size.y, ppp),
                );
                let pill = snap::rect(Rect::from_min_size(at, size), ppp);
                BoxStyle {
                    background: v.bg_muted,
                    radius: Corners::all(v.radius_sm),
                    ..BoxStyle::default()
                }
                .paint(ui.painter(), pill);
                text::paint(
                    ui.painter(),
                    Rect::from_min_size(at + vec2(4.0, 1.0), vec2(tw, ih)),
                    &label,
                    &s.inherited,
                );
            }
            // The control.
            let mut x = line.left() + label_w + ROW_GAP;
            let right = line.right();
            let reset_style = reset_style(v);
            let reset = panel_button::size(&ctx, RESET, &reset_style);
            let rid = id.with(("token", row.key.name()));
            if model::is_color_kind(row.kind) {
                let cr = Rect::from_min_size(
                    pos2(x, line.top() + snap::centre(field_h, COLOR_H, ppp)),
                    vec2(COLOR_W, COLOR_H),
                );
                if let Some(hex) =
                    color_input(ui, cr, rid.with("color"), &model::to_hex(&row.value), props)
                {
                    editor.set_token(row.key, model::from_hex(&hex, row.kind));
                }
                x += COLOR_W + CONTROL_GAP;
            }
            let field_rect = Rect::from_min_max(
                pos2(x, line.top()),
                pos2(right - reset.x - CONTROL_GAP, line.bottom()),
            );
            if row.kind == TokenKind::Choice {
                let choices = row.key.spec().choices;
                let options: Vec<String> = choices
                    .iter()
                    .map(|c| model::choice_label(locale, row.key, c))
                    .collect();
                let selected = choices.iter().position(|c| *c == row.value).unwrap_or(0);
                let ev = select::select(
                    ui,
                    field_rect,
                    rid.with("select"),
                    &SelectProps {
                        options: &options,
                        selected,
                        disabled: &[],
                        label: &row.label,
                        vars: v,
                        look: None,
                    },
                );
                if let Some(k) = ev.changed {
                    editor.set_token(row.key, choices[k].to_owned());
                }
            } else {
                let mut value = row.value.clone();
                // Only the plain fields carry the kind's example.
                let placeholder = if model::is_color_kind(row.kind) {
                    ""
                } else {
                    row.kind.example()
                };
                let ev = text_input::text_input(
                    ui,
                    field_rect,
                    rid.with("field"),
                    &mut value,
                    &TextInputProps {
                        style: &field,
                        kind: InputKind::Text,
                        placeholder,
                        label: &row.label,
                        max_len: None,
                        enabled: true,
                        vars: v,
                        dark: props.dark,
                    },
                );
                if ev.changed {
                    editor.set_token(row.key, value);
                }
            }
            let reset_rect = Rect::from_min_size(
                pos2(
                    right - reset.x,
                    line.top() + snap::centre(field_h, reset.y, ppp),
                ),
                reset,
            );
            if faded_button(
                ui,
                reset_rect,
                rid.with("reset"),
                RESET,
                &reset_style,
                row.overridden,
                0.3,
                props,
            ) {
                editor.reset_token(row.key);
            }
        }
        Block::TokenIssue(i) => {
            if let Some((text, colour)) = &m.rows[*i].issue {
                let style = TextStyle::new(11.0, 400).color(colour.to_color32());
                text::paint(ui.painter(), r, text, &style);
            }
        }
        Block::Help => {
            let help = i18n::translate(locale, "themeEditor.customCssHelp");
            text::paint(ui.painter(), r, &help, &s.help);
        }
        Block::Css => {
            let mut css = std::mem::take(&mut editor.css);
            let mut top = editor.css_scroll;
            css_field(ui, r, id.with("css"), &mut css, &mut top, s, props);
            editor.css = css;
            editor.css_scroll = top;
        }
        Block::Issue(text, colour) => {
            let colour = if colour.a == 0.0 {
                v.fg_subtle
            } else {
                *colour
            };
            let style = TextStyle::new(11.0, 400).color(colour.to_color32());
            text::paint(ui.painter(), r, text, &style);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn meta_grid(
    ui: &mut Ui,
    r: Rect,
    id: Id,
    s: &Styles,
    m: &Model,
    editor: &mut ThemeEditor,
    props: &DialogProps,
    field_h: f32,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let locale = props.locale;
    let field = settings_style(v);
    let tracks = meta_tracks(r.width(), ppp);
    let cols = tracks.len();
    let label_h = line_h(&ctx, &s.label);
    let cell_h = label_h + FIELD_GAP + field_h;
    let cell = |k: usize| {
        let (row, col) = (k / cols, k % cols);
        let (left, w) = tracks[col];
        Rect::from_min_size(
            pos2(r.left() + left, r.top() + row as f32 * (cell_h + META_GAP)),
            vec2(w, cell_h),
        )
    };
    let labelled = |ui: &mut Ui, k: usize, key: &str| {
        let c = cell(k);
        text::paint(
            ui.painter(),
            Rect::from_min_size(c.min, vec2(c.width(), label_h)),
            &i18n::translate(locale, key),
            &s.label,
        );
        Rect::from_min_size(
            pos2(c.left(), c.top() + label_h + FIELD_GAP),
            vec2(c.width(), field_h),
        )
    };
    let texts: [(&str, usize); 3] = [
        ("themeEditor.name", 0),
        ("themeEditor.author", 1),
        ("themeEditor.version", 2),
    ];
    for (key, k) in texts {
        let rect = labelled(ui, k, key);
        let label = i18n::translate(locale, key);
        let value = match k {
            0 => &mut editor.name,
            1 => &mut editor.author,
            _ => &mut editor.version,
        };
        text_input::text_input(
            ui,
            rect,
            id.with(("meta", k)),
            value,
            &TextInputProps {
                style: &field,
                kind: InputKind::Text,
                placeholder: "",
                label: &label,
                max_len: Some(64),
                enabled: true,
                vars: v,
                dark: props.dark,
            },
        );
    }
    let rect = labelled(ui, 3, "themeEditor.base");
    let selected = m
        .base_ids
        .iter()
        .position(|b| *b == editor.base)
        .unwrap_or(0);
    let ev = select::select(
        ui,
        rect,
        id.with(("meta", 3)),
        &SelectProps {
            options: &m.base_labels,
            selected,
            disabled: &[],
            label: &i18n::translate(locale, "themeEditor.base"),
            vars: v,
            look: None,
        },
    );
    if let Some(k) = ev.changed {
        editor.base = m.base_ids[k].clone();
    }
    let rect = labelled(ui, 4, "themeEditor.colorScheme");
    let schemes = [
        i18n::translate(locale, "theme.dark"),
        i18n::translate(locale, "theme.light"),
    ];
    let ev = select::select(
        ui,
        rect,
        id.with(("meta", 4)),
        &SelectProps {
            options: &schemes,
            selected: usize::from(editor.color_scheme == ColorScheme::Light),
            disabled: &[],
            label: &i18n::translate(locale, "themeEditor.colorScheme"),
            vars: v,
            look: None,
        },
    );
    if let Some(k) = ev.changed {
        editor.color_scheme = if k == 1 {
            ColorScheme::Light
        } else {
            ColorScheme::Dark
        };
    }
    // `.checkbox-field`: the box over its label, centred, at the cell's end.
    let c = cell(5);
    let top = c.bottom() - (CHECK + CHECK_GAP + label_h);
    let check = Rect::from_min_size(
        pos2(c.left() + snap::centre(c.width(), CHECK, ppp), top),
        vec2(CHECK, CHECK),
    );
    let mut glass = editor.glass;
    checkbox(ui, check, id.with(("meta", 5)), &mut glass, props);
    editor.glass = glass;
    let label = i18n::translate(locale, "themeEditor.glass");
    let label_w = snap::content_width(text::measure(&ctx, &label, &s.label).unwrap_or(0.0), ppp)
        .min(c.width());
    let label_rect = Rect::from_min_size(
        pos2(
            c.left() + snap::centre(c.width(), label_w, ppp),
            top + CHECK + CHECK_GAP,
        ),
        vec2(label_w, label_h),
    );
    // A click on the label toggles the box (label activation).
    if ui
        .interact(label_rect, id.with(("meta", 5, "label")), Sense::click())
        .clicked()
    {
        editor.glass = !editor.glass;
    }
    text::paint(ui.painter(), label_rect, &label, &s.label);
}
