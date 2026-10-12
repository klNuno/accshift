//! Typed style values, filled from the resolved theme once per theme change.
//!
//! One field per CSS custom property a ported component reads, named after
//! the property (`--bg-card` is `bg_card`). Components never look a variable
//! up by name at draw time. A component port adds the fields it needs here,
//! with the property in its doc line, and fills them in [`Vars::new`].

use accshift_theme::ResolvedTheme;

use crate::theme::Rgba;
use crate::ui::css;
use crate::ui::paint::{BoxShadow, Gradient};

#[derive(Clone, Debug, PartialEq)]
pub struct Vars {
    /// `--bg`: the window fill `.app-shell` paints (translucent on glass).
    pub bg: Rgba,
    /// `--bg-solid`: `--bg` at full opacity (macOS title bar).
    pub bg_solid: Rgba,
    /// `--bg-image`: the theme's gradients over `--bg`, first on top. Empty
    /// for `none`.
    pub bg_image: Vec<Gradient>,
    /// `--bg-card`.
    pub bg_card: Rgba,
    /// `--bg-card-hover`.
    pub bg_card_hover: Rgba,
    /// `--card-bg-image`: the theme's card gradients, first on top. Empty
    /// for `none`.
    pub card_bg_image: Vec<Gradient>,
    /// `--bg-muted`.
    pub bg_muted: Rgba,
    /// `--bg-elevated`.
    pub bg_elevated: Rgba,
    /// `--bg-input`.
    pub bg_input: Rgba,
    /// `--bg-overlay`.
    pub bg_overlay: Rgba,
    /// `--fg`.
    pub fg: Rgba,
    /// `--fg-muted`.
    pub fg_muted: Rgba,
    /// `--fg-subtle`.
    pub fg_subtle: Rgba,
    /// `--border`.
    pub border: Rgba,
    /// `--border-width`, CSS px.
    pub border_width: f32,
    /// `--border-style` is `none`: borders drawn with it vanish.
    pub border_style_none: bool,
    /// `--danger`.
    pub danger: Rgba,
    /// `--success`.
    pub success: Rgba,
    /// `--elevation-medium`, first on top.
    pub elevation_medium: Vec<BoxShadow>,
    /// `--accent`.
    pub accent: Rgba,
    /// `--accent-text`.
    pub accent_text: Rgba,
    /// `--motion-page-entrance`, seconds.
    pub motion_page_entrance: f64,
    /// `--motion-card-entrance`, seconds.
    pub motion_card_entrance: f64,
    /// `--motion-avatar-fade`, seconds.
    pub motion_avatar_fade: f64,
    /// `--radius-sm`, CSS px.
    pub radius_sm: f32,
    /// `--radius-md`, CSS px (`--grid-card-radius`).
    pub radius_md: f32,
    /// `--avatar-radius`, CSS px (`var(--avatar-radius-default, 6px)`; a
    /// percentage falls back to 6 px until the full account card lands).
    pub avatar_radius: f32,
    /// `--density-scale`.
    pub density_scale: f32,
    /// `--motion-avatar-defer`, seconds.
    pub motion_avatar_defer: f64,
    /// `--line-height` (unitless), the body's `line-height`.
    pub line_height: f32,
    /// `--overlay-blur`, CSS px: the dialogs' `backdrop-filter: blur()`.
    pub overlay_blur: f32,
}

/// `accshift_theme::Rgba` (channels 0 to 1) in the painter's colour type
/// (channels 0 to 255, alpha 0 to 1).
pub fn rgba(c: accshift_theme::Rgba) -> Rgba {
    Rgba::new(c.r * 255.0, c.g * 255.0, c.b * 255.0, c.a)
}

impl Vars {
    pub fn new(theme: &ResolvedTheme) -> Vars {
        let color = |name: &str| theme.color(name).map(rgba).unwrap_or(Rgba::TRANSPARENT);
        let images = |name: &str| -> Vec<Gradient> {
            theme
                .substitute(name)
                .map(|value| {
                    let value = value.trim();
                    if value.is_empty() || value == "none" {
                        return Vec::new();
                    }
                    css::split_top(value, ',')
                        .into_iter()
                        .filter_map(css::gradient)
                        .collect()
                })
                .unwrap_or_default()
        };
        let seconds = |name: &str, default: f32| {
            f64::from(theme.duration_ms(name).unwrap_or(default)) / 1000.0
        };
        let density_scale = theme
            .number("--density-scale")
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or(1.0);
        Vars {
            bg: color("--bg"),
            bg_solid: color("--bg-solid"),
            bg_image: images("--bg-image"),
            bg_card: color("--bg-card"),
            bg_card_hover: color("--bg-card-hover"),
            card_bg_image: images("--card-bg-image"),
            bg_muted: color("--bg-muted"),
            bg_elevated: color("--bg-elevated"),
            bg_input: color("--bg-input"),
            bg_overlay: color("--bg-overlay"),
            fg: color("--fg"),
            fg_muted: color("--fg-muted"),
            fg_subtle: color("--fg-subtle"),
            border: color("--border"),
            border_width: theme.length_px("--border-width").unwrap_or(1.0),
            border_style_none: theme
                .substitute("--border-style")
                .is_some_and(|style| style.trim() == "none"),
            danger: color("--danger"),
            success: color("--success"),
            elevation_medium: theme
                .substitute("--elevation-medium")
                .and_then(|value| css::shadows(&value))
                .unwrap_or_default(),
            accent: color("--accent"),
            accent_text: color("--accent-text"),
            motion_page_entrance: seconds("--motion-page-entrance", 200.0),
            motion_card_entrance: seconds("--motion-card-entrance", 260.0),
            motion_avatar_fade: seconds("--motion-avatar-fade", 220.0),
            radius_sm: theme.length_px("--radius-sm").unwrap_or(4.0),
            radius_md: theme.length_px("--radius-md").unwrap_or(8.0),
            avatar_radius: theme.length_px("--avatar-radius").unwrap_or(6.0),
            density_scale,
            motion_avatar_defer: seconds("--motion-avatar-defer", 380.0),
            line_height: theme
                .number("--line-height")
                .filter(|n| n.is_finite() && *n > 0.0)
                .unwrap_or(1.5),
            overlay_blur: theme.length_px("--overlay-blur").unwrap_or(4.0),
        }
    }

    /// `--grid-card-width`: `calc(100px * var(--density-scale))`.
    pub fn grid_card_width(&self) -> f32 {
        100.0 * self.density_scale
    }

    /// `--grid-card-min-height`: `calc(136px * var(--density-scale))`.
    pub fn grid_card_min_height(&self) -> f32 {
        136.0 * self.density_scale
    }

    /// `--grid-card-padding`: `calc(8px * var(--density-scale))`.
    pub fn grid_card_padding(&self) -> f32 {
        8.0 * self.density_scale
    }

    /// `--grid-card-avatar-size`: `calc(68px * var(--density-scale))`.
    pub fn grid_card_avatar_size(&self) -> f32 {
        68.0 * self.density_scale
    }

    /// `var(--border-width) var(--border-style) var(--border)` as a width,
    /// zero when the style is `none`.
    pub fn border_px(&self) -> f32 {
        if self.border_style_none {
            0.0
        } else {
            self.border_width
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accshift_theme::{theme_application, ThemeRegistry};

    fn vars(id: &str, opacity: f64, backdrop: bool) -> Vars {
        let registry = ThemeRegistry::new();
        let theme = registry.theme(Some(id));
        Vars::new(&ResolvedTheme::new(&theme_application(
            &theme, opacity, backdrop,
        )))
    }

    #[test]
    fn glass_dark_fills_the_shell_values() {
        let v = vars("glass-dark", 85.0, true);
        // Same numbers the theme crate's doc example checks.
        let c = v.bg_card;
        let alpha = (c.a * 255.0).round();
        assert_eq!(
            [c.r.round(), c.g.round(), c.b.round(), alpha],
            [19.0, 19.0, 24.0, 166.0]
        );
        assert!(
            v.bg.a > 0.0 && v.bg.a < 1.0,
            "glass window fill is translucent"
        );
        assert_eq!(v.bg_solid.a, 1.0);
        assert!(v.bg_image.is_empty());
        assert_eq!(v.border_px(), 1.0);
        assert!((v.motion_page_entrance - 0.2).abs() < 1e-9);
        assert!((v.motion_card_entrance - 0.26).abs() < 1e-9);
        assert!((v.motion_avatar_fade - 0.22).abs() < 1e-9);
        assert_eq!(v.grid_card_width(), 100.0);
        assert_eq!(v.grid_card_min_height(), 136.0);
        assert_eq!(v.radius_md, 8.0);
        assert_eq!(v.avatar_radius, 6.0);
    }

    #[test]
    fn dark_is_opaque() {
        let v = vars("dark", 100.0, true);
        assert_eq!(v.bg.a, 1.0);
        assert_eq!(
            v.fg.to_color32().to_srgba_unmultiplied(),
            [250, 250, 250, 255]
        );
    }
}
