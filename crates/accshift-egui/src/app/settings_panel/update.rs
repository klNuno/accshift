//! The General tab's about card: the running version, the "Check for
//! updates" button and what the update flow is doing, in plain words. The
//! flow lives in the app (`app::updates`); its state is copied here each
//! frame. A ready update installs through the title bar's own path.

use accshift_app::i18n::{translate, translate_with, Locale, TranslationValue};

use super::SettingsAction;

/// The longest error the card shows, in characters; a longer one is cut.
const ERROR_CHARS: usize = 120;

/// Where the update flow stands, as the card shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Debug, mock and scene runs: the updater is off.
    #[default]
    Unavailable,
    /// No check has ended yet.
    Idle,
    Checking,
    /// A newer version was found and downloads.
    Downloading(String),
    UpToDate,
    /// The newer version is downloaded and waits for a restart.
    Ready(String),
    Installing,
    /// The check or the download failed, with the error text.
    Failed(String),
    /// Installed, but the relaunch failed: restart by hand.
    RestartNeeded,
}

/// What the card's button and the line under it say.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateRow {
    pub label: String,
    pub enabled: bool,
    pub action: SettingsAction,
    /// Empty when there is nothing to say.
    pub notice: String,
    /// The notice is an error.
    pub error: bool,
}

/// The card's "Version 2.0.0" line.
pub fn version_line(version: &str, locale: Locale) -> String {
    translate_with(
        locale,
        "settings.appVersion",
        &[("version", TranslationValue::Str(version.to_string()))],
    )
}

/// The button and the notice `status` asks for. Only an idle flow, or one
/// that ended, takes a new check; a ready update offers its restart.
pub fn update_row(status: &UpdateStatus, locale: Locale) -> UpdateRow {
    let t = |key: &str| translate(locale, key);
    let with = |key: &str, name: &str, value: &str| {
        translate_with(
            locale,
            key,
            &[(name, TranslationValue::Str(value.to_string()))],
        )
    };
    let check = |enabled: bool, notice: String, error: bool| UpdateRow {
        label: t("settings.checkUpdates"),
        enabled,
        action: SettingsAction::CheckUpdates,
        notice,
        error,
    };
    match status {
        UpdateStatus::Unavailable => check(false, t("settings.updatesUnavailable"), false),
        UpdateStatus::Idle => check(true, String::new(), false),
        UpdateStatus::Checking => check(false, t("settings.updateChecking"), false),
        UpdateStatus::Downloading(version) => check(
            false,
            with("settings.updateDownloading", "version", version),
            false,
        ),
        UpdateStatus::UpToDate => check(true, t("settings.updateUpToDate"), false),
        UpdateStatus::Failed(error) => check(
            true,
            with("settings.updateFailed", "error", &short_error(error)),
            true,
        ),
        UpdateStatus::RestartNeeded => check(false, t("update.restartManually"), false),
        UpdateStatus::Ready(version) => UpdateRow {
            label: t("update.restartToApply"),
            enabled: true,
            action: SettingsAction::ApplyUpdate,
            notice: with("update.readyToastVersion", "version", version),
            error: false,
        },
        UpdateStatus::Installing => UpdateRow {
            label: t("update.ctaInstalling"),
            enabled: false,
            action: SettingsAction::ApplyUpdate,
            notice: String::new(),
            error: false,
        },
    }
}

/// The first line of `error`, cut at [`ERROR_CHARS`].
fn short_error(error: &str) -> String {
    let line = error.lines().next().unwrap_or("").trim();
    if line.chars().count() <= ERROR_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(ERROR_CHARS).collect();
    format!("{}...", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_and_finished_checks_take_a_new_check() {
        for status in [
            UpdateStatus::Idle,
            UpdateStatus::UpToDate,
            UpdateStatus::Failed("offline".into()),
        ] {
            let row = update_row(&status, Locale::En);
            assert!(row.enabled, "{status:?}");
            assert_eq!(row.action, SettingsAction::CheckUpdates);
            assert_eq!(row.label, "Check for updates");
        }
    }

    #[test]
    fn a_running_or_impossible_check_disables_the_button() {
        for status in [
            UpdateStatus::Unavailable,
            UpdateStatus::Checking,
            UpdateStatus::Downloading("2.0.1".into()),
            UpdateStatus::Installing,
            UpdateStatus::RestartNeeded,
        ] {
            assert!(!update_row(&status, Locale::En).enabled, "{status:?}");
        }
    }

    #[test]
    fn each_state_says_what_it_is_in_plain_words() {
        let notice = |status: UpdateStatus| {
            let row = update_row(&status, Locale::En);
            (row.notice, row.error)
        };
        assert_eq!(notice(UpdateStatus::Idle), (String::new(), false));
        assert_eq!(
            notice(UpdateStatus::Unavailable),
            ("Updates are off in this build.".into(), false)
        );
        assert_eq!(
            notice(UpdateStatus::Checking),
            ("Checking for updates...".into(), false)
        );
        assert_eq!(
            notice(UpdateStatus::UpToDate),
            ("accshift is up to date.".into(), false)
        );
        assert_eq!(
            notice(UpdateStatus::Downloading("2.0.1".into())),
            ("Update 2.0.1 found, downloading...".into(), false)
        );
        assert_eq!(
            notice(UpdateStatus::Failed("operation timed out".into())),
            ("Update check failed: operation timed out".into(), true)
        );
        assert_eq!(
            notice(UpdateStatus::RestartNeeded),
            (
                "Update installed. Restart accshift to finish.".into(),
                false
            )
        );
    }

    /// A ready update offers the title bar's restart, not a second check.
    #[test]
    fn a_ready_update_offers_its_restart() {
        let row = update_row(&UpdateStatus::Ready("2.0.1".into()), Locale::En);
        assert_eq!(row.label, "Restart to apply update");
        assert_eq!(row.action, SettingsAction::ApplyUpdate);
        assert!(row.enabled);
        assert_eq!(row.notice, "Update 2.0.1 ready");
        let row = update_row(&UpdateStatus::Installing, Locale::En);
        assert_eq!(row.label, "Installing...");
        assert!(!row.enabled);
    }

    #[test]
    fn a_long_error_keeps_its_first_line_and_is_cut() {
        let long = format!("{}\nsecond line", "x".repeat(300));
        let row = update_row(&UpdateStatus::Failed(long), Locale::En);
        assert!(!row.notice.contains("second line"));
        assert!(row.notice.ends_with("..."), "{}", row.notice);
        assert!(row.notice.chars().count() < 160, "{}", row.notice);
    }

    #[test]
    fn every_locale_has_the_card_strings_filled() {
        let statuses = [
            UpdateStatus::Unavailable,
            UpdateStatus::Idle,
            UpdateStatus::Checking,
            UpdateStatus::Downloading("2.0.1".into()),
            UpdateStatus::UpToDate,
            UpdateStatus::Ready("2.0.1".into()),
            UpdateStatus::Installing,
            UpdateStatus::Failed("boom".into()),
            UpdateStatus::RestartNeeded,
        ];
        for locale in accshift_app::i18n::LOCALES {
            let version = version_line("2.0.0", locale);
            assert!(version.contains("2.0.0"), "{locale:?}: {version}");
            for key in [
                "settings.about",
                "settings.appVersion",
                "settings.checkUpdates",
                "settings.updatesUnavailable",
                "settings.updateChecking",
                "settings.updateDownloading",
                "settings.updateUpToDate",
                "settings.updateFailed",
            ] {
                assert!(
                    accshift_app::i18n::dictionary(locale).contains_key(key),
                    "{locale:?}: {key}"
                );
            }
            for status in &statuses {
                let row = update_row(status, locale);
                assert!(!row.label.is_empty(), "{locale:?} {status:?}");
                for text in [&row.label, &row.notice] {
                    assert!(!text.contains('{'), "{locale:?} {status:?}: {text}");
                    assert!(!text.contains("settings."), "{locale:?}: {text}");
                }
            }
            let failed = update_row(&UpdateStatus::Failed("boom".into()), locale);
            assert!(failed.notice.contains("boom"), "{locale:?}");
        }
    }
}
