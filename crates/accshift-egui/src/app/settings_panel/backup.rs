//! The Privacy tab's backup card: what it draws, and the confirm a picked
//! backup asks. The jobs and the restore live in the app
//! (`app::backup_flow`), so closing the panel never drops them.

use accshift_app::actions::dialogs::ConfirmConfig;
use accshift_app::i18n::{translate, Locale, TranslationValue};
use accshift_app::text::Text;
use accshift_app::types::BackupPreview;

/// What the backup card draws, copied from the app each frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackupState {
    /// A job or a restore is under way: the buttons are disabled.
    pub busy: bool,
    /// The last outcome, shown under the buttons.
    pub notice: String,
    /// The notice is an error.
    pub error: bool,
}

/// The confirm a picked backup asks, with what it holds.
pub fn restore_confirm(preview: &BackupPreview, locale: Locale) -> ConfirmConfig {
    let date = accshift_app::time::format_absolute_date_time(
        Some(preview.created_at as f64),
        locale,
        &accshift_app::time::system_zone(),
    );
    let settings = translate(
        locale,
        if preview.has_settings {
            "settings.backupSettingsIncluded"
        } else {
            "settings.backupSettingsMissing"
        },
    );
    let n = |count: usize| TranslationValue::Str(count.to_string());
    ConfirmConfig {
        title: Text::key("settings.backupRestoreTitle"),
        message: Text::with(
            "settings.backupRestoreMessage",
            [
                ("date", TranslationValue::Str(date)),
                (
                    "version",
                    TranslationValue::Str(preview.app_version.clone()),
                ),
                ("folders", n(preview.folders)),
                ("notes", n(preview.notes)),
                ("colors", n(preview.colors)),
                ("personas", n(preview.personas)),
                ("themes", n(preview.themes)),
                ("platforms", n(preview.descriptors)),
                ("settings", TranslationValue::Str(settings)),
            ],
        ),
        confirm_label: Some(Text::key("settings.backupRestoreConfirm")),
        confirm_color: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview() -> BackupPreview {
        BackupPreview {
            path: "C:/backups/accshift-backup-2026-11-15.json".into(),
            format: 1,
            created_at: 1_791_000_000,
            app_version: "2.0.0".into(),
            folders: 3,
            notes: 5,
            colors: 2,
            personas: 1,
            themes: 4,
            descriptors: 0,
            has_settings: true,
            digest: String::new(),
        }
    }

    #[test]
    fn the_restore_confirm_lists_what_the_file_holds_and_that_sessions_stay_out() {
        for locale in accshift_app::i18n::LOCALES {
            let config = restore_confirm(&preview(), locale);
            let message = config.message.resolve(locale);
            for part in ["2.0.0", "3", "5", "4", "user/backups"] {
                assert!(message.contains(part), "{locale:?}: {part} in {message}");
            }
            assert!(!message.contains('{'), "{locale:?}: {message}");
        }
        let en = restore_confirm(&preview(), Locale::En)
            .message
            .resolve(Locale::En);
        assert!(en.contains("Saved sessions are not included"), "{en}");
        assert!(en.contains("Settings: included"), "{en}");
    }
}
