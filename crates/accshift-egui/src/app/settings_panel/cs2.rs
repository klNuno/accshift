//! The Steam tab's CS2 bridge card: it reads the bridge settings when the
//! tab is shown, tests a configured link, saves its fields, and loses its
//! state when the tab goes.

use std::time::Instant;

use accshift_app::actions::toasts::{ToastQueue, ToastType};
use accshift_app::error::BackendResult;
use accshift_app::i18n::{Locale, TranslationValue};
use accshift_app::jobs::{Jobs, Pending};
use accshift_app::types::{Cs2BridgeSettings, Cs2BridgeTestResult};

use super::jobs::{cancelled, drain_landed, settle};
use super::state::SettingsPanel;
use super::{toast, SettingsAction};

/// The link test's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cs2Status {
    #[default]
    Unknown,
    Testing,
    Ok,
    Fail,
}

/// The card's state while the Steam tab is shown.
#[derive(Default)]
pub struct Cs2Form {
    pub enabled: bool,
    pub url: String,
    pub token_input: String,
    pub token_configured: bool,
    /// The settings were read (or failed to): the toggle and blur saves
    /// wait for it.
    pub loaded: bool,
    pub url_error: bool,
    pub status: Cs2Status,
    pub status_detail: String,
    pub status_error: String,
    load_job: Option<Pending<BackendResult<Cs2BridgeSettings>>>,
    test_job: Option<Pending<BackendResult<Cs2BridgeTestResult>>>,
    /// The saves in flight, with the token each sent.
    save_jobs: Vec<(Pending<BackendResult<()>>, Option<String>)>,
}

impl Cs2Form {
    /// A fresh card, reading the bridge settings.
    fn mount(jobs: &Jobs) -> Cs2Form {
        Cs2Form {
            load_job: Some(jobs.run(|b| b.cs2_bridge_get_settings())),
            ..Cs2Form::default()
        }
    }

    /// Tests the link; a blank link only resets the status.
    fn run_test(&mut self, jobs: &Jobs) {
        if self.url.trim().is_empty() {
            self.status = Cs2Status::Unknown;
            self.status_detail.clear();
            self.status_error.clear();
            return;
        }
        self.status = Cs2Status::Testing;
        self.status_error.clear();
        self.test_job = Some(jobs.run(|b| b.cs2_bridge_test()));
    }

    /// Saves the toggle and the link, and the token when one is given.
    fn save(&mut self, jobs: &Jobs, token: Option<String>) {
        let (enabled, url, sent) = (self.enabled, self.url.trim().to_string(), token.clone());
        let job = jobs.run(move |b| b.cs2_bridge_set_settings(enabled, &url, sent.as_deref()));
        self.save_jobs.push((job, token));
    }

    /// One of the card's actions. The ones that save wait until the
    /// settings were read.
    pub(super) fn apply(&mut self, jobs: &Jobs, action: SettingsAction) {
        match action {
            SettingsAction::Cs2Toggle if self.loaded => {
                self.enabled = !self.enabled;
                self.save(jobs, None);
            }
            SettingsAction::Cs2Url(text) => self.url = text,
            SettingsAction::Cs2UrlBlur if self.loaded => self.save(jobs, None),
            SettingsAction::Cs2Test => self.run_test(jobs),
            SettingsAction::Cs2Token(text) => self.token_input = text,
            SettingsAction::Cs2TokenBlur if self.loaded && !self.token_input.trim().is_empty() => {
                let token = self.token_input.clone();
                self.save(jobs, Some(token));
            }
            SettingsAction::Cs2ClearToken => self.save(jobs, Some(String::new())),
            _ => {}
        }
    }

    /// The bridge settings landed. When they could not be read the defaults
    /// stay and a save surfaces the error. An enabled bridge with a link is
    /// tested at once.
    fn poll_load(&mut self, jobs: &Jobs) {
        let Some(landed) = settle(&mut self.load_job) else {
            return;
        };
        if let Some(settings) = landed.and_then(Result::ok) {
            self.enabled = settings.enabled;
            self.url = settings.url;
            self.token_configured = settings.token_configured;
            if self.enabled && !self.url.is_empty() {
                self.run_test(jobs);
            }
        }
        self.loaded = true;
    }

    /// The link test answered: the account count and latency, or why it
    /// failed.
    fn poll_test(&mut self, locale: Locale) {
        let Some(landed) = settle(&mut self.test_job) else {
            return;
        };
        let error = match landed.unwrap_or_else(cancelled) {
            Ok(r) if r.ok => {
                self.status = Cs2Status::Ok;
                self.status_detail = accshift_app::i18n::translate_with(
                    locale,
                    "settings.cs2BridgeStatusOk",
                    &[
                        ("count", TranslationValue::Num(r.account_count as f64)),
                        ("ms", TranslationValue::Num(r.latency_ms as f64)),
                    ],
                );
                self.status_error.clear();
                return;
            }
            Ok(r) => r.error.unwrap_or_default(),
            Err(error) => error.message,
        };
        self.status = Cs2Status::Fail;
        self.status_detail = accshift_app::i18n::translate(locale, "settings.cs2BridgeStatusFail");
        self.status_error = error;
    }

    /// The saves that landed. Each success tests the link again; the result
    /// is the bridge's state when one succeeded, for the account cards.
    fn poll_saves(&mut self, jobs: &Jobs, toasts: &mut ToastQueue, now: Instant) -> Option<bool> {
        let mut reload = None;
        for ((_, token), result) in drain_landed(&mut self.save_jobs, |(job, _)| job.poll()) {
            if result.is_err() {
                self.url_error = true;
                toast(
                    toasts,
                    "settings.cs2BridgeSaveFailed",
                    ToastType::Error,
                    now,
                );
                continue;
            }
            self.url_error = false;
            if let Some(token) = token {
                self.token_configured = !token.trim().is_empty();
                if self.token_input == token {
                    self.token_input.clear();
                }
            }
            reload = Some(self.enabled);
            self.run_test(jobs);
        }
        reload
    }
}

impl SettingsPanel {
    /// The card exists while the Steam tab is shown: it mounts fresh and
    /// loses its state when the tab goes.
    pub(super) fn sync_cs2_mount(&mut self, jobs: &Jobs) {
        if !self.steam_mounted() {
            self.cs2 = None;
        } else if self.cs2.is_none() {
            self.cs2 = Some(Cs2Form::mount(jobs));
        }
    }

    /// Polls the card's calls; `Some` when a save asks the account cards to
    /// drop their bridge data (and read it again when it holds `true`, the
    /// bridge being on).
    pub(super) fn tick_cs2(
        &mut self,
        jobs: &Jobs,
        toasts: &mut ToastQueue,
        now: Instant,
    ) -> Option<bool> {
        let locale = self.draft.language;
        let form = self.cs2.as_mut()?;
        form.poll_load(jobs);
        form.poll_test(locale);
        form.poll_saves(jobs, toasts, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn a_completed_token_save_keeps_a_newer_draft() {
        for sent in ["saved token", ""] {
            let backend = accshift_app::mock::MockBackend::from_json(
                include_str!("../../../parity/data/empty.json"),
                None,
            )
            .unwrap();
            let (wake, woke) = mpsc::channel();
            let jobs = Jobs::new(Arc::new(backend), 1, move || {
                let _ = wake.send(());
            });
            let (release, delayed) = mpsc::channel();
            let job = jobs.run(move |_| {
                delayed.recv().unwrap();
                Ok(())
            });
            let mut form = Cs2Form {
                token_input: sent.into(),
                save_jobs: vec![(job, Some(sent.into()))],
                ..Cs2Form::default()
            };
            let mut toasts = ToastQueue::new();
            assert_eq!(form.poll_saves(&jobs, &mut toasts, Instant::now()), None);
            form.apply(&jobs, SettingsAction::Cs2Token("new draft".into()));
            release.send(()).unwrap();
            woke.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(
                form.poll_saves(&jobs, &mut toasts, Instant::now()),
                Some(false)
            );
            assert_eq!(form.token_input, "new draft");
            assert_eq!(form.token_configured, !sent.is_empty());
        }
    }
}
