//! The Privacy tab's TcNo import card: what it draws, the confirm a picked
//! folder asks and the report once imported. The import itself runs in the
//! app (`app::tcno_flow`), behind the backup's boundary.

use accshift_app::actions::dialogs::{ConfirmConfig, CONFIRM_ACCENT};
use accshift_app::i18n::{Locale, TranslationValue};
use accshift_app::text::Text;
use accshift_app::types::{TcNoImportReport, TcNoPreview, TcNoWarning, TcNoWarningKind};

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

/// The preview adds nothing: no new account, note, session or place in an
/// order.
pub fn tcno_preview_is_empty(preview: &TcNoPreview) -> bool {
    preview.accounts_new == 0
        && preview.notes_new == 0
        && preview.snapshots_new == 0
        && preview.order_new == 0
}

/// The text key naming a reason something was left out, with `{count}`.
fn reason_key(kind: TcNoWarningKind) -> &'static str {
    use TcNoWarningKind::*;
    match kind {
        VaultEncrypted => "settings.tcnoWarn.vaultEncrypted",
        EpicEglv2Skipped => "settings.tcnoWarn.epicEglv2Skipped",
        UnsupportedIdsFormat => "settings.tcnoWarn.unsupportedIdsFormat",
        CorruptFile => "settings.tcnoWarn.corruptFile",
        InvalidAccountId => "settings.tcnoWarn.invalidAccountId",
        IdentityNotPortable => "settings.tcnoWarn.identityNotPortable",
        SnapshotIncomplete => "settings.tcnoWarn.snapshotIncomplete",
        SnapshotFailed => "settings.tcnoWarn.snapshotFailed",
    }
}

/// What was left out, one count per reason in the order they first came;
/// the details are in the app log. `None` without warnings.
fn tcno_warnings(warnings: &[TcNoWarning], locale: Locale) -> Option<String> {
    let mut counts: Vec<(TcNoWarningKind, usize)> = Vec::new();
    for warning in warnings {
        match counts.iter_mut().find(|(kind, _)| *kind == warning.kind) {
            Some((_, count)) => *count += 1,
            None => counts.push((warning.kind, 1)),
        }
    }
    if counts.is_empty() {
        return None;
    }
    let list = counts
        .into_iter()
        .map(|(kind, count)| Text::with(reason_key(kind), [("count", n(count))]).resolve(locale))
        .collect::<Vec<_>>()
        .join("; ");
    Some(
        Text::with(
            "settings.tcnoWarnings",
            [("list", TranslationValue::Str(list))],
        )
        .resolve(locale),
    )
}

/// `text`, then what was left out.
fn with_warnings(text: String, warnings: &[TcNoWarning], locale: Locale) -> String {
    match tcno_warnings(warnings, locale) {
        Some(left_out) => format!("{text} {left_out}"),
        None => text,
    }
}

/// What the card says when a folder adds nothing, with what it left out.
pub fn tcno_nothing(warnings: &[TcNoWarning], locale: Locale) -> String {
    with_warnings(
        accshift_app::i18n::translate(locale, "settings.tcnoNothing"),
        warnings,
        locale,
    )
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
                ("order", n(preview.order_new)),
                ("capture", n(preview.to_capture)),
                ("warnings", n(preview.warnings.len())),
            ],
        ),
        confirm_label: Some(Text::key("settings.tcnoConfirm")),
        // An import only adds: not the destructive red.
        confirm_color: Some(CONFIRM_ACCENT.to_string()),
    }
}

/// What the card says once the import ran, with what it left out.
pub fn tcno_report(report: &TcNoImportReport, locale: Locale) -> String {
    let text = Text::with(
        "settings.tcnoImported",
        [
            ("accounts", n(report.accounts_imported)),
            ("notes", n(report.notes_imported)),
            ("sessions", n(report.snapshots_imported)),
            ("order", n(report.order_entries_added)),
            ("capture", n(report.to_capture)),
            ("skipped", n(report.accounts_skipped)),
        ],
    )
    .resolve(locale);
    with_warnings(text, &report.warnings, locale)
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
    fn the_tcno_confirm_is_not_styled_as_destructive() {
        // An import only adds: its button is the accent one, not the red.
        let config = tcno_confirm(&preview());
        assert_eq!(
            config.confirm_color.as_deref(),
            Some(accshift_app::actions::dialogs::CONFIRM_ACCENT)
        );
    }

    fn warning(kind: TcNoWarningKind) -> TcNoWarning {
        TcNoWarning {
            platform: "jagex".into(),
            kind,
            detail: "Skipped account 'Main'".into(),
        }
    }

    #[test]
    fn a_folder_that_only_brings_an_order_is_not_empty() {
        let preview = TcNoPreview {
            order_new: 2,
            ..TcNoPreview::default()
        };
        assert!(!tcno_preview_is_empty(&preview));
        let message = tcno_confirm(&preview).message.resolve(Locale::En);
        assert!(message.contains("TcNo order: 2"), "{message}");
    }

    #[test]
    fn the_tcno_report_counts_what_was_left_out_per_reason() {
        let report = TcNoImportReport {
            accounts_imported: 1,
            order_entries_added: 3,
            warnings: vec![
                warning(TcNoWarningKind::IdentityNotPortable),
                warning(TcNoWarningKind::IdentityNotPortable),
                warning(TcNoWarningKind::SnapshotFailed),
            ],
            ..TcNoImportReport::default()
        };
        let text = tcno_report(&report, Locale::En);
        assert!(text.contains("TcNo order: 3"), "{text}");
        assert!(
            text.contains("accounts accshift cannot switch: 2"),
            "{text}"
        );
        assert!(text.contains("could not be copied: 1"), "{text}");
        assert!(text.contains("logs"), "{text}");
        // Only the counts: the details stay in the log.
        assert!(!text.contains("Main"), "{text}");
        for locale in accshift_app::i18n::LOCALES {
            let text = tcno_report(&report, locale);
            assert!(!text.contains('{'), "{locale:?}: {text}");
            assert!(!text.contains("settings."), "{locale:?}: {text}");
        }
    }

    #[test]
    fn every_reason_has_its_text_in_every_language() {
        use TcNoWarningKind::*;
        let kinds = [
            VaultEncrypted,
            EpicEglv2Skipped,
            UnsupportedIdsFormat,
            CorruptFile,
            InvalidAccountId,
            IdentityNotPortable,
            SnapshotIncomplete,
            SnapshotFailed,
        ];
        let warnings: Vec<_> = kinds.into_iter().map(warning).collect();
        for locale in accshift_app::i18n::LOCALES {
            let text = tcno_nothing(&warnings, locale);
            assert!(!text.contains("settings."), "{locale:?}: {text}");
            assert!(!text.contains('{'), "{locale:?}: {text}");
            assert_eq!(text.matches('1').count(), kinds.len(), "{locale:?}: {text}");
        }
        assert_eq!(
            tcno_nothing(&[], Locale::En),
            accshift_app::i18n::translate(Locale::En, "settings.tcnoNothing")
        );
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
