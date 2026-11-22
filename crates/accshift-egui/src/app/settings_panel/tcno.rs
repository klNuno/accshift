//! The Privacy tab's TcNo import card: what it draws, the confirm a picked
//! folder asks and the report once imported. The import itself runs in the
//! app (`app::tcno_flow`), behind the backup's boundary.

use accshift_app::actions::dialogs::ConfirmConfig;
use accshift_app::i18n::{Locale, TranslationValue};
use accshift_app::text::Text;
use accshift_app::types::{TcNoImportReport, TcNoPreview};

/// What the TcNo card draws, copied from the app each frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TcnoState {
    /// TcNo Account Switcher only runs on Windows: no card elsewhere.
    pub available: bool,
    /// A pick, an import or a backup is under way: the button is disabled.
    pub busy: bool,
    /// The last outcome, shown under the button.
    pub notice: String,
    /// The notice is an error.
    pub error: bool,
}

fn n(count: usize) -> TranslationValue {
    TranslationValue::Str(count.to_string())
}

/// The preview adds nothing: no new account, note or session.
pub fn tcno_preview_is_empty(preview: &TcNoPreview) -> bool {
    preview.accounts_new == 0 && preview.notes_new == 0 && preview.snapshots_new == 0
}

/// The confirm a picked TcNo folder asks, with what the import would add.
pub fn tcno_confirm(preview: &TcNoPreview) -> ConfirmConfig {
    ConfirmConfig {
        title: Text::key("settings.tcnoConfirmTitle"),
        message: Text::with(
            "settings.tcnoConfirmMessage",
            [
                ("accounts", n(preview.accounts_new)),
                ("existing", n(preview.accounts_existing)),
                ("notes", n(preview.notes_new)),
                ("sessions", n(preview.snapshots_new)),
                ("capture", n(preview.to_capture)),
                ("warnings", n(preview.warnings.len())),
            ],
        ),
        confirm_label: Some(Text::key("settings.tcnoConfirm")),
        confirm_color: None,
    }
}

/// What the card says once the import ran.
pub fn tcno_report(report: &TcNoImportReport, locale: Locale) -> String {
    Text::with(
        "settings.tcnoImported",
        [
            ("accounts", n(report.accounts_imported)),
            ("notes", n(report.notes_imported)),
            ("sessions", n(report.snapshots_imported)),
            ("capture", n(report.to_capture)),
            ("skipped", n(report.accounts_skipped)),
        ],
    )
    .resolve(locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview() -> TcNoPreview {
        TcNoPreview {
            path: "C:/TcNo".into(),
            accounts_new: 7,
            accounts_existing: 2,
            notes_new: 3,
            snapshots_new: 4,
            to_capture: 5,
            ..TcNoPreview::default()
        }
    }

    #[test]
    fn the_tcno_confirm_and_report_fill_every_count_in_every_language() {
        let report = TcNoImportReport {
            accounts_imported: 7,
            accounts_skipped: 2,
            notes_imported: 3,
            snapshots_imported: 4,
            to_capture: 5,
            ..TcNoImportReport::default()
        };
        for locale in accshift_app::i18n::LOCALES {
            let config = tcno_confirm(&preview());
            let message = config.message.resolve(locale);
            for part in ["7", "2", "3", "4", "5"] {
                assert!(message.contains(part), "{locale:?}: {part} in {message}");
            }
            assert!(!message.contains('{'), "{locale:?}: {message}");
            let text = tcno_report(&report, locale);
            for part in ["7", "2", "3", "4", "5"] {
                assert!(text.contains(part), "{locale:?}: {part} in {text}");
            }
            assert!(!text.contains('{'), "{locale:?}: {text}");
            for key in [
                "settings.tcno",
                "settings.tcnoHint",
                "settings.tcnoImport",
                "settings.tcnoDialog",
                "settings.tcnoNothing",
                "settings.tcnoReadFailed",
                "settings.tcnoImportFailed",
                "settings.tcnoRestartNeeded",
                "settings.tcnoConfirmTitle",
                "settings.tcnoConfirm",
            ] {
                let text = accshift_app::i18n::translate(locale, key);
                assert_ne!(text, key, "{locale:?}: {key} is missing");
            }
        }
    }

    #[test]
    fn a_folder_with_nothing_new_is_empty() {
        assert!(!tcno_preview_is_empty(&preview()));
        let mut preview = preview();
        preview.accounts_new = 0;
        preview.notes_new = 0;
        preview.snapshots_new = 0;
        assert!(tcno_preview_is_empty(&preview));
    }
}
