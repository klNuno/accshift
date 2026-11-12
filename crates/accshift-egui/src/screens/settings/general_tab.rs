//! The cards of the General tab: appearance, theme, account display,
//! performance, integrations and (on Windows) system.

use accshift_app::i18n::{Locale, LANGUAGE_OPTIONS};
use accshift_app::settings::AnimationsMode;

use super::{tr, CardSpec, Control, Item, View};
use crate::app::platforms::RuntimeOs;
use crate::app::settings_panel::{NumericField, SettingFlag, SettingsAction, SettingsPanel};

/// The animation modes in the order of their select, with their labels.
const ANIMATIONS: [(AnimationsMode, &str); 3] = [
    (AnimationsMode::System, "settings.animationsSystem"),
    (AnimationsMode::On, "settings.animationsOn"),
    (AnimationsMode::Off, "settings.animationsOff"),
];

/// An `accshift://` link reaches the app as a second launch's argument. macOS
/// sends it as an Apple Event instead, which the app does not listen for, and
/// the macOS bundle registers no scheme: no toggle for a link that cannot
/// arrive.
fn deep_links_reachable(os: RuntimeOs) -> bool {
    os != RuntimeOs::Macos
}

pub(super) fn cards(panel: &SettingsPanel, view: &View, l: Locale) -> Vec<CardSpec> {
    let d = &panel.draft;
    let t = |k: &str| tr(l, k);
    let toggle = |key: &str, desc: &str, on: bool, flag: SettingFlag| Item::Toggle {
        label: t(key),
        description: if desc.is_empty() {
            String::new()
        } else {
            t(desc)
        },
        enabled: on,
        disabled: false,
        action: SettingsAction::Toggle(flag),
    };
    let language = Item::Field {
        label: t("settings.language"),
        hint: None,
        control: Control::Select {
            options: LANGUAGE_OPTIONS.iter().map(|o| t(o.label_key)).collect(),
            selected: LANGUAGE_OPTIONS
                .iter()
                .position(|o| o.code == d.language)
                .unwrap_or(0),
            actions: LANGUAGE_OPTIONS
                .iter()
                .map(|o| SettingsAction::Language(o.code))
                .collect(),
            disabled: Vec::new(),
        },
    };
    let zoom = Item::Field {
        label: format!("{} - {}%", t("settings.uiZoom"), panel.ui_scale.input),
        hint: None,
        control: Control::Slider {
            min: 75.0,
            max: 150.0,
            step: 5.0,
            field: NumericField::UiScale,
            commit_on_input: false,
        },
    };
    let animations = Item::Field {
        label: t("settings.animations"),
        hint: None,
        control: Control::Select {
            options: ANIMATIONS.iter().map(|(_, key)| t(key)).collect(),
            selected: ANIMATIONS
                .iter()
                .position(|(mode, _)| *mode == d.animations)
                .unwrap_or(0),
            actions: ANIMATIONS
                .iter()
                .map(|(mode, _)| SettingsAction::Animations(*mode))
                .collect(),
            disabled: Vec::new(),
        },
    };

    // No background opacity slider under a glass theme.
    let glass = view
        .themes
        .iter()
        .find(|th| th.id == d.theme_id)
        .is_some_and(|th| th.glass);
    let mut theme = vec![Item::Title(t("settings.theme")), Item::ThemeGrid];
    if !glass {
        theme.push(Item::Field {
            label: format!(
                "{} - {}%",
                t("settings.backgroundOpacity"),
                d.background_opacity
            ),
            hint: None,
            control: Control::Slider {
                min: 0.0,
                max: 100.0,
                step: 5.0,
                field: NumericField::BgOpacity,
                commit_on_input: true,
            },
        });
    }

    let mut integrations = vec![Item::TitleRow(
        t("settings.integrations"),
        t("settings.help"),
    )];
    if deep_links_reachable(view.runtime_os) {
        integrations.push(toggle(
            "settings.deepLinks",
            "settings.deepLinksHint",
            d.deep_links_enabled,
            SettingFlag::DeepLinks,
        ));
    }
    integrations.push(toggle(
        "settings.cliEnabled",
        "settings.cliEnabledHint",
        d.cli_enabled,
        SettingFlag::Cli,
    ));

    let mut cards = vec![
        vec![
            Item::Title(t("settings.appearance")),
            language,
            zoom,
            animations,
        ],
        theme,
        vec![
            Item::Title(t("settings.accountDisplay")),
            toggle(
                "settings.showNotesUnderCards",
                "",
                d.account_display.show_card_notes_inline,
                SettingFlag::ShowCardNotesInline,
            ),
            toggle(
                "settings.expandedFolders",
                "",
                d.account_display.expanded_folders,
                SettingFlag::ExpandedFolders,
            ),
        ],
        vec![
            Item::Title(t("settings.performance")),
            toggle(
                "settings.suspendGraphicsWhenMinimized",
                "",
                d.suspend_graphics_when_minimized,
                SettingFlag::SuspendGraphicsWhenMinimized,
            ),
            toggle(
                "settings.minimizeOnAccountSwitch",
                "",
                d.minimize_on_account_switch,
                SettingFlag::MinimizeOnAccountSwitch,
            ),
        ],
        integrations,
    ];
    // Launch at sign-in and close-to-tray are Windows-only.
    if view.runtime_os == RuntimeOs::Windows {
        cards.push(vec![
            Item::Title(t("settings.system")),
            toggle(
                "settings.startWithSystem",
                "settings.startWithSystemHint",
                d.start_with_system,
                SettingFlag::StartWithSystem,
            ),
            toggle(
                "settings.closeToTray",
                "settings.closeToTrayHint",
                d.close_to_tray,
                SettingFlag::CloseToTray,
            ),
        ]);
    }
    cards
        .into_iter()
        .map(|items| CardSpec { items, wide: false })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::deep_links_reachable;
    use crate::app::platforms::RuntimeOs;

    #[test]
    fn macos_has_no_deep_link_toggle() {
        assert!(deep_links_reachable(RuntimeOs::Windows));
        assert!(deep_links_reachable(RuntimeOs::Linux));
        assert!(!deep_links_reachable(RuntimeOs::Macos));
    }
}
