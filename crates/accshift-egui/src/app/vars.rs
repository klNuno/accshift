//! Typed style values, filled from the resolved theme once per theme change.
//!
//! One field per CSS custom property a ported component reads, named after
//! the property (`--bg-card` is `bg_card`). Components never look a variable
//! up by name at draw time. A component port adds the fields it needs here,
//! with the property in its doc line, and fills them in [`Vars::new`].

use accshift_theme::ResolvedTheme;

use crate::theme::Rgba;
use crate::ui::css;
use crate::ui::paint::Gradient;

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
    /// `--bg-muted`.
    pub bg_muted: Rgba,
    /// `--bg-elevated`.
    pub bg_elevated: Rgba,
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
    /// `--accent`.
    pub accent: Rgba,
    /// `--accent-text`.
    pub accent_text: Rgba,
    /// `--motion-page-entrance`, seconds.
    pub motion_page_entrance: f64,
}

/// `accshift_theme::Rgba` (channels 0 to 1) in the painter's colour type
/// (channels 0 to 255, alpha 0 to 1).
pub fn rgba(c: accshift_theme::Rgba) -> Rgba {
    Rgba::new(c.r * 255.0, c.g * 255.0, c.b * 255.0, c.a)
}

impl Vars {
    pub fn new(theme: &ResolvedTheme) -> Vars {
        let color = |name: &str| theme.color(name).map(rgba).unwrap_or(Rgba::TRANSPARENT);
        let bg_image = theme
            .substitute("--bg-image")
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
            .unwrap_or_default();
        Vars {
            bg: color("--bg"),
            bg_solid: color("--bg-solid"),
            bg_image,
            bg_card: color("--bg-card"),
            bg_muted: color("--bg-muted"),
            bg_elevated: color("--bg-elevated"),
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
            accent: color("--accent"),
            accent_text: color("--accent-text"),
            motion_page_entrance: f64::from(
                theme.duration_ms("--motion-page-entrance").unwrap_or(200.0),
            ) / 1000.0,
        }
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
