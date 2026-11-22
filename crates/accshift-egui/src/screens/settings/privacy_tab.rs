//! The cards of the Privacy tab: privacy, security (the PIN lock), backup,
//! the TcNo import and telemetry.

use accshift_app::i18n::Locale;
use accshift_app::settings::StreamerMode;

use super::{tr, CardSpec, Control, InputField, Item};
use crate::app::settings_panel::{NumericField, SettingFlag, SettingsAction, SettingsPanel};
use crate::ui::components::controls::text_input::InputKind;

pub(super) fn cards(panel: &SettingsPanel, l: Locale) -> Vec<CardSpec> {
    let d = &panel.draft;
    let t = |k: &str| tr(l, k);
    let mut cards = vec![
        CardSpec {
            wide: false,
            items: vec![
                Item::Title(t("settings.privacy")),
                Item::Field {
                    label: t("settings.inactivityTimeout"),
                    hint: Some(format!(
                        "({}, {})",
                        t("settings.unitSeconds"),
                        t("settings.zeroDisabled")
                    )),
                    control: Control::Input {
                        kind: InputKind::Number,
                        field: InputField::Numeric(NumericField::InactivityBlur),
                        placeholder: String::new(),
                        enabled: true,
                    },
                },
                Item::Toggle {
                    label: t("settings.streamerMode"),
                    description: t("settings.streamerModeHint"),
                    enabled: d.streamer_mode == StreamerMode::Auto,
                    disabled: false,
                    action: SettingsAction::Toggle(SettingFlag::StreamerMode),
                },
                Item::Button {
                    label: t("settings.replayOnboarding"),
                    enabled: true,
                    action: SettingsAction::ReplayOnboarding,
                },
                Item::Hint(t("settings.replayOnboardingHint"), false),
            ],
        },
        CardSpec {
            wide: false,
            items: security(panel, l),
        },
        backup(panel, l),
    ];
    cards.extend(tcno(panel, l));
    cards.extend(telemetry(panel, l));
    cards
}

/// The PIN toggle, then the field the PIN's state asks for: the current
/// PIN when the panel is PIN-guarded, else a new PIN while one is enabled
/// or being set up.
fn security(panel: &SettingsPanel, l: Locale) -> Vec<Item> {
    let t = |k: &str| tr(l, k);
    let pin_field = |label: &str, field: InputField, enabled: bool| Item::Field {
        label: t(label),
        hint: None,
        control: Control::Input {
            kind: InputKind::Password,
            field,
            placeholder: t("settings.pinPlaceholder"),
            enabled,
        },
    };
    let mut items = vec![
        Item::Title(t("settings.security")),
        Item::Toggle {
            label: t("settings.pinLockOnAfk"),
            description: String::new(),
            enabled: panel.pin_toggle_on(),
            disabled: false,
            action: SettingsAction::TogglePin,
        },
        Item::Hint(t("settings.pinScreenLockOnly"), false),
    ];
    if panel.pin_guarded() {
        items.push(pin_field(
            "settings.pinCurrentCode",
            InputField::CurrentPin,
            !panel.current_pin_busy(),
        ));
        items.push(if panel.current_pin_error.is_empty() {
            Item::FieldHint(t("settings.pinCurrentHint"), false)
        } else {
            Item::FieldHint(panel.current_pin_error.clone(), true)
        });
    } else if panel.draft.pin_enabled || panel.pin_setup_pending {
        items.push(pin_field("settings.pinCode", InputField::Pin, true));
        items.push(Item::FieldHint(
            t("settings.pinRequiredAfterInactivity"),
            false,
        ));
    }
    items
}

/// The backup card: what the file holds and leaves out, export, restore,
/// then the last outcome.
fn backup(panel: &SettingsPanel, l: Locale) -> CardSpec {
    let t = |k: &str| tr(l, k);
    let busy = panel.backup.busy;
    let mut items = vec![
        Item::Title(t("settings.backup")),
        Item::Hint(t("settings.backupHint"), false),
        Item::Button {
            label: t("settings.backupExport"),
            enabled: !busy,
            action: SettingsAction::ExportBackup,
        },
        Item::Button {
            label: t("settings.backupImport"),
            enabled: !busy,
            action: SettingsAction::ImportBackup,
        },
    ];
    if !panel.backup.notice.is_empty() {
        items.push(Item::Hint(panel.backup.notice.clone(), panel.backup.error));
    }
    CardSpec { wide: false, items }
}

/// The TcNo import card, on Windows only: what it adds and leaves alone,
/// the folder picker, then the last outcome.
fn tcno(panel: &SettingsPanel, l: Locale) -> Option<CardSpec> {
    if !panel.tcno.available {
        return None;
    }
    let t = |k: &str| tr(l, k);
    let mut items = vec![
        Item::Title(t("settings.tcno")),
        Item::Hint(t("settings.tcnoHint"), false),
        Item::Button {
            label: t("settings.tcnoImport"),
            enabled: !panel.tcno.busy,
            action: SettingsAction::ImportTcno,
        },
    ];
    if !panel.tcno.notice.is_empty() {
        items.push(Item::Hint(panel.tcno.notice.clone(), panel.tcno.error));
    }
    Some(CardSpec { wide: false, items })
}

/// The telemetry card, two columns wide: its modes once loaded, a retry
/// after a failed load, no card otherwise.
fn telemetry(panel: &SettingsPanel, l: Locale) -> Option<CardSpec> {
    let t = |k: &str| tr(l, k);
    let Some(tel) = &panel.telemetry else {
        return panel.telemetry_error.then(|| CardSpec {
            wide: true,
            items: vec![
                Item::Title(t("settings.telemetry")),
                Item::Hint(t("settings.telemetryLoadFailed"), false),
                Item::Button {
                    label: t("common.retry"),
                    enabled: true,
                    action: SettingsAction::RetryTelemetryLoad,
                },
            ],
        });
    };
    let mut items = vec![
        Item::Title(t("settings.telemetry")),
        Item::Link(
            t("settings.telemetryLearnMore"),
            SettingsAction::OpenTelemetryDoc,
        ),
        Item::Toggle {
            label: t("settings.telemetryModeA"),
            description: t("settings.telemetryModeAHint"),
            enabled: tel.mode_a_enabled,
            disabled: false,
            action: SettingsAction::ToggleTelemetryA,
        },
        Item::Toggle {
            label: t("settings.telemetryModeB"),
            description: t("settings.telemetryModeBHint"),
            enabled: tel.mode_b_enabled,
            disabled: panel.mode_b_busy,
            action: SettingsAction::ToggleTelemetryB,
        },
    ];
    if tel.forget_pending {
        items.push(Item::Hint(t("settings.telemetryDeletePendingHint"), false));
        items.push(Item::Button {
            label: t("settings.telemetryRetryDelete"),
            enabled: !panel.mode_b_busy,
            action: SettingsAction::RetryTelemetryDeletion,
        });
    }
    if tel.install_id_set {
        items.push(Item::Button {
            label: t("settings.telemetryExport"),
            enabled: !panel.export_busy,
            action: SettingsAction::ExportTelemetry,
        });
    }
    Some(CardSpec { wide: true, items })
}
