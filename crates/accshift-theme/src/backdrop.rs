//! Port of the decision half of `src/lib/theme/backdrop.ts`: which OS effect
//! a theme asks for behind the transparent window. Applying it is the app's
//! job (window-vibrancy on Windows and macOS, nothing on Linux).

/// macOS vibrancy material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MacEffect {
    HudWindow,
    Popover,
    UnderWindowBackground,
}

/// What the window behind the UI should be.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WindowBackdrop {
    /// Plain transparent window, every effect cleared.
    Off,
    Glass {
        /// Material id: `liquid-glass` for any theme that extends it, else
        /// the theme id.
        material: String,
        /// Windows Acrylic. Liquid Glass uses none: the window stays purely
        /// transparent and any acrylic left by a previous theme is cleared.
        windows_acrylic: bool,
        /// Acrylic tint, RGBA. Only the SWCA acrylic path honours it.
        acrylic_tint: Option<[u8; 4]>,
        mac_effect: MacEffect,
        /// Keep DWM drawing the backdrop while the window is unfocused
        /// (`set_keep_backdrop_active`): on for acrylic only.
        keep_backdrop_active: bool,
    },
}

impl WindowBackdrop {
    /// The memo key of `applyWindowBackdrop`: re-apply only when it changes.
    pub fn key(&self) -> String {
        match self {
            WindowBackdrop::Off => "off".to_owned(),
            WindowBackdrop::Glass { material, .. } => format!("backdrop:{material}"),
        }
    }
}

/// `applyWindowBackdrop(glass, themeId, liquid)` without the OS call.
/// `liquid` defaults to `theme_id == "liquid-glass"` in TypeScript; pass
/// `AppTheme::liquid`.
pub fn window_backdrop(glass: bool, theme_id: &str, liquid: bool) -> WindowBackdrop {
    if !glass {
        return WindowBackdrop::Off;
    }
    let material = if liquid { "liquid-glass" } else { theme_id };
    let acrylic = !liquid;
    WindowBackdrop::Glass {
        material: material.to_owned(),
        windows_acrylic: acrylic,
        acrylic_tint: (material == "glass-dark").then_some([0, 0, 0, 150]),
        mac_effect: match material {
            "glass-dark" => MacEffect::HudWindow,
            "glass-light" => MacEffect::Popover,
            _ => MacEffect::UnderWindowBackground,
        },
        keep_backdrop_active: acrylic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_material_per_theme() {
        assert_eq!(window_backdrop(false, "dark", false), WindowBackdrop::Off);
        assert_eq!(window_backdrop(false, "dark", false).key(), "off");
        let glass_dark = window_backdrop(true, "glass-dark", false);
        assert_eq!(
            glass_dark,
            WindowBackdrop::Glass {
                material: "glass-dark".to_owned(),
                windows_acrylic: true,
                acrylic_tint: Some([0, 0, 0, 150]),
                mac_effect: MacEffect::HudWindow,
                keep_backdrop_active: true,
            }
        );
        let my_liquid = window_backdrop(true, "my-liquid", true);
        assert_eq!(my_liquid.key(), "backdrop:liquid-glass");
        assert!(matches!(
            my_liquid,
            WindowBackdrop::Glass {
                windows_acrylic: false,
                keep_backdrop_active: false,
                mac_effect: MacEffect::UnderWindowBackground,
                ..
            }
        ));
        assert!(matches!(
            window_backdrop(true, "glass-light", false),
            WindowBackdrop::Glass {
                acrylic_tint: None,
                mac_effect: MacEffect::Popover,
                ..
            }
        ));
    }
}
