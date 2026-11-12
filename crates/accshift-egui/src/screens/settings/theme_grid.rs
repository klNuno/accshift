//! The theme picker of the Theme card: a grid of swatches, one per theme,
//! and the Customize, Import and Export buttons under it.

use accshift_theme::{is_built_in_theme, AppTheme, TokenKey};
use egui::accesskit::Role;
use egui::{pos2, vec2, Context, Id, Key, Rect, Sense, Ui, WidgetInfo};

use super::{tr, Ctx, FIELD_GAP};
use crate::app::settings_panel::SettingsAction;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::components::controls::button::{self, ButtonProps};
use crate::ui::components::controls::{focus_ring, take_focus, T120};
use crate::ui::css;
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners, Gradient};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// The gap between swatches, and between the action buttons.
const GAP: f32 = 8.0;
/// The space above the grid, and above the action row.
const MARGIN_TOP: f32 = 4.0;

/// The grid's columns: as many 86 px swatches as fit, at least one.
fn columns(width: f32) -> usize {
    (((width + GAP) / 94.0).floor() as usize).max(1)
}

/// A swatch: a 2 px border, 8 px of padding on top and 6 px at the bottom,
/// the preview (23 px inside its 1 px border), a 6 px gap and the 12 px
/// label. Borders are laid out floored to device pixels.
fn swatch_h(ppp: f32) -> f32 {
    55.0 + 2.0 * snap::border_width(2.0, ppp) + 2.0 * snap::border_width(1.0, ppp)
}

/// An action button: a 26.5 px box inside a 1 px border.
fn action_h(ppp: f32) -> f32 {
    26.5 + 2.0 * snap::border_width(1.0, ppp)
}

/// The top of the action row for a grid starting at `top`: the swatch rows,
/// the field's gap and the row's own margin.
fn actions_top(top: f32, rows: usize, ppp: f32) -> f32 {
    top + rows as f32 * swatch_h(ppp) + rows.saturating_sub(1) as f32 * GAP + FIELD_GAP + MARGIN_TOP
}

/// The picker's height for `themes` swatches at `width`.
pub(super) fn height(ctx: &Context, themes: usize, width: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    let rows = themes.div_ceil(columns(width));
    actions_top(MARGIN_TOP, rows, ppp) + action_h(ppp)
}

/// Paints the picker into `r`. Picking a swatch selects its theme.
pub(super) fn paint(ui: &mut Ui, r: Rect, id: Id, c: &Ctx) -> Vec<SettingsAction> {
    let ppp = ui.ctx().pixels_per_point();
    let cols = columns(r.width());
    let tracks = snap::fr_tracks(r.width(), cols, GAP, ppp);
    let top = r.top() + MARGIN_TOP;
    let mut out = Vec::new();
    for (i, theme) in c.view.themes.iter().enumerate() {
        let (left, col_w) = tracks[i % cols];
        let rect = Rect::from_min_size(
            pos2(
                r.left() + left,
                top + (swatch_h(ppp) + GAP) * (i / cols) as f32,
            ),
            vec2(col_w, swatch_h(ppp)),
        );
        if swatch_button(ui, rect, id.with(("swatch", theme.id.as_str())), c, theme) {
            out.push(SettingsAction::Theme(theme.id.clone()));
        }
    }
    let rows = c.view.themes.len().div_ceil(cols);
    out.extend(action_row(ui, r, actions_top(top, rows, ppp), id, c));
    out
}

/// One swatch as a button. True when activated.
fn swatch_button(ui: &mut Ui, rect: Rect, id: Id, c: &Ctx, theme: &AppTheme) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let name = if theme.is_custom {
        theme.display_name.clone()
    } else {
        tr(c.locale, theme.label_key)
    };
    let response = ui.interact(rect, id, Sense::click());
    let selected = c.panel.draft.theme_id == theme.id;
    response.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, &name));
    let focused = take_focus(ui, &response);
    let s = Swatch::of(theme);
    let (border, shadows) = s.ring(selected, response.hovered());
    let border = anim::transition(&ctx, id.with("border"), border, &T120);
    s.paint(ui, rect, border, shadows, &name, selected);
    focus_ring(
        ui,
        snap::rect(rect, ppp),
        8.0,
        2.0,
        &c.view.vars,
        c.focus,
        focused,
    );
    let keyed = focused && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space));
    response.clicked() || keyed
}

/// Customize (Edit for a custom theme), Import and Export, sharing the
/// row's width.
fn action_row(ui: &mut Ui, r: Rect, top: f32, id: Id, c: &Ctx) -> Vec<SettingsAction> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    let customize = if is_built_in_theme(&c.panel.draft.theme_id) {
        "settings.themeCustomize"
    } else {
        "settings.themeEdit"
    };
    let actions = [
        (customize, SettingsAction::CustomizeTheme),
        ("settings.themeImport", SettingsAction::ImportTheme),
        ("settings.themeExport", SettingsAction::ExportTheme),
    ];
    let style = button::theme_action_style(v);
    let labels = actions.each_ref().map(|(key, _)| tr(c.locale, key));
    let mins = labels.each_ref().map(|l| button::size(&ctx, l, &style).x);
    let widths = flex_equal(r.width() - 2.0 * GAP, &mins);
    let mut out = Vec::new();
    let mut x = r.left();
    for (k, ((_, action), label)) in actions.into_iter().zip(&labels).enumerate() {
        let rect = Rect::from_min_size(pos2(x, top), vec2(widths[k], action_h(ppp)));
        x += widths[k] + GAP;
        let props = ButtonProps {
            label,
            aria_label: None,
            style: &style,
            enabled: true,
            vars: v,
            focus: c.focus,
        };
        if button::button(ui, rect, id.with(("theme-action", k)), &props) {
            out.push(action);
        }
    }
    out
}

/// `flex: 1` items sharing `avail`: equal shares, except that an item never
/// shrinks under its min-content width; frozen items leave the rest to the
/// others, as the flex algorithm resolves it.
fn flex_equal<const N: usize>(avail: f32, mins: &[f32; N]) -> [f32; N] {
    let mut frozen = [false; N];
    loop {
        let free: f32 = avail - (0..N).filter(|&i| frozen[i]).map(|i| mins[i]).sum::<f32>();
        let open = frozen.iter().filter(|f| !**f).count().max(1);
        let share = free / open as f32;
        let mut changed = false;
        for i in 0..N {
            if !frozen[i] && mins[i] > share {
                frozen[i] = true;
                changed = true;
            }
        }
        if !changed {
            return std::array::from_fn(|i| if frozen[i] { mins[i] } else { share });
        }
    }
}

/// Background layers, preview card, foreground and border of a theme's
/// swatch.
struct Swatch {
    bg: Rgba,
    images: Vec<Gradient>,
    card: Rgba,
    fg: Rgba,
    border: Rgba,
}

fn token_color(theme: &AppTheme, key: TokenKey) -> Rgba {
    css::color(theme.tokens.get(key)).unwrap_or(Rgba::TRANSPARENT)
}

impl Swatch {
    fn of(theme: &AppTheme) -> Swatch {
        let rgb: Vec<f32> = theme
            .tokens
            .get(TokenKey::BgRgb)
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        let bg_rgb = match rgb.as_slice() {
            [r, g, b] => Rgba::new(*r, *g, *b, 1.0),
            _ => Rgba::new(0.0, 0.0, 0.0, 1.0),
        };
        let card = token_color(theme, TokenKey::BgCard);
        let fg = token_color(theme, TokenKey::Fg);
        let border = token_color(theme, TokenKey::Border);
        if !theme.glass {
            return Swatch {
                bg: bg_rgb,
                images: Vec::new(),
                card,
                fg,
                border,
            };
        }
        // A glass theme: a veil of its background over a fixed wallpaper
        // gradient, its card and border see-through.
        let liquid = theme.id == "liquid-glass";
        let veil = if liquid { 0.2 } else { 0.6 };
        let card_pct = if liquid { 0.22 } else { 0.72 };
        let veil_css = format!(
            "linear-gradient(rgb({} {} {} / {veil}), rgb({} {} {} / {veil}))",
            bg_rgb.r, bg_rgb.g, bg_rgb.b, bg_rgb.r, bg_rgb.g, bg_rgb.b
        );
        let images = [
            veil_css.as_str(),
            "linear-gradient(135deg, #8a63a8, #4f6d9e 48%, #a8825f)",
        ]
        .iter()
        .filter_map(|g| css::gradient(g))
        .collect();
        Swatch {
            bg: Rgba::TRANSPARENT,
            images,
            card: card.mix(card_pct, Rgba::TRANSPARENT),
            fg,
            border: border.mix(0.45, Rgba::TRANSPARENT),
        }
    }

    /// The border colour and shadows the swatch eases toward: a ring in
    /// its foreground when selected, a lift when hovered, none otherwise.
    fn ring(&self, selected: bool, hovered: bool) -> (Rgba, Vec<BoxShadow>) {
        if selected {
            (
                self.fg,
                vec![BoxShadow {
                    offset: egui::Vec2::ZERO,
                    blur: 0.0,
                    spread: 1.0,
                    color: self.fg.mix(0.3, Rgba::TRANSPARENT),
                    inset: false,
                }],
            )
        } else if hovered {
            (
                self.fg.mix(0.25, Rgba::TRANSPARENT),
                vec![BoxShadow {
                    offset: vec2(0.0, 2.0),
                    blur: 8.0,
                    spread: 0.0,
                    color: Rgba::new(0.0, 0.0, 0.0, 0.18),
                    inset: false,
                }],
            )
        } else {
            (Rgba::TRANSPARENT, Vec::new())
        }
    }

    /// The swatch box, the preview card with its two text bars, and the
    /// theme's name under it.
    fn paint(
        &self,
        ui: &Ui,
        rect: Rect,
        border: Rgba,
        shadows: Vec<BoxShadow>,
        name: &str,
        selected: bool,
    ) {
        let ppp = ui.ctx().pixels_per_point();
        BoxStyle {
            background: self.bg,
            images: self.images.clone(),
            radius: Corners::all(8.0),
            border: Border::all(snap::border_width(2.0, ppp), border),
            shadows,
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(rect, ppp));
        let edge = snap::border_width(2.0, ppp);
        let inner = Rect::from_min_max(
            rect.min + vec2(8.0 + edge, 8.0 + edge),
            pos2(rect.right() - 8.0 - edge, rect.bottom() - 6.0 - edge),
        );
        let preview_border = snap::border_width(1.0, ppp);
        let preview =
            Rect::from_min_size(inner.min, vec2(inner.width(), 23.0 + 2.0 * preview_border));
        BoxStyle {
            background: self.card,
            radius: Corners::all(4.0),
            border: Border::all(snap::border_width(1.0, ppp), self.border),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(preview, ppp));
        let bar_pad = 6.0 + preview_border;
        let bar_w = preview.width() - 2.0 * bar_pad;
        for (k, (w, a)) in [(bar_w, 0.35), (bar_w * 0.6, 0.2)].into_iter().enumerate() {
            let bar = Rect::from_min_size(
                preview.min + vec2(bar_pad, bar_pad + 7.0 * k as f32),
                vec2(w, 4.0),
            );
            BoxStyle {
                background: self.fg,
                radius: Corners::all(2.0),
                opacity: a,
                ..BoxStyle::default()
            }
            .paint(ui.painter(), snap::rect(bar, ppp));
        }
        let label_style = TextStyle::new(10.0, 600)
            .line_height(LineHeight::Number(1.2))
            .nowrap(true)
            .align(TextAlign::Center)
            .color(
                self.fg
                    .alpha(self.fg.a * if selected { 1.0 } else { 0.7 })
                    .to_color32(),
            );
        text::paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(inner.left(), preview.bottom() + 6.0),
                vec2(inner.width(), 12.0),
            ),
            name,
            &label_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::flex_equal;

    #[test]
    fn flex_equal_shares_and_freezes_wide_items() {
        assert_eq!(flex_equal(255.0, &[60.0, 60.0, 60.0]), [85.0, 85.0, 85.0]);
        // "Personnaliser" is wider than a third: it keeps its min-content
        // width and the others split the rest.
        assert_eq!(flex_equal(255.0, &[92.0, 60.0, 62.0]), [92.0, 81.5, 81.5]);
    }
}
