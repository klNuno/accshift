//! The first-run telemetry onboarding: `useOnboardingTour.svelte.ts` (the
//! tour state of [`OnboardingTour`]) and the script half of
//! `TelemetryOnboarding.svelte` (steps, the rejection timers, the consent
//! save). `ui::components::onboarding` draws it.

use accshift_app::actions::onboarding::{mock_accounts, OnboardingTour};
use accshift_app::actions::AppEffect;
use accshift_app::backend::Backend;
use accshift_app::error::BackendResult;
use accshift_app::i18n::Locale;
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::platforms::PlatformAccount;
use egui::Vec2;

/// `REJECT_FADE_DELAY_MS`: the "enough" row fades the modal after this.
pub const REJECT_FADE_DELAY: f64 = 1.2;
/// `REJECT_TOTAL_MS`: then saves the choice.
pub const REJECT_TOTAL: f64 = 2.0;
/// `TELEMETRY_DOC_URL`.
pub const TELEMETRY_DOC_URL: &str =
    "https://github.com/klNuno/accshift/blob/main/docs/analytics.md";
/// `FEATURES.length`.
pub const FEATURE_COUNT: usize = 4;

/// `Step`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    Welcome,
    Features,
    Deal,
}

/// What the dialog reported.
#[derive(Clone, Debug, PartialEq)]
pub enum OnboardingEvent {
    /// `handleSkip`.
    Skip,
    /// `goFeatures` (Show me around, Back from the deal step).
    GoFeatures,
    /// `prevFeature`.
    PrevFeature,
    /// `nextFeature`.
    NextFeature,
    /// A legend dot.
    Feature(usize),
    /// `handleEnough`, with the clip's box (`gifSize`) when it was drawn.
    Enough(Option<Vec2>),
    /// `handleDeal`.
    Deal,
    /// `handleRefuse`.
    Refuse,
    /// `retryPersist`.
    Retry,
    /// The persist error's "later": `onComplete` without a save.
    Later,
    /// `openDoc`.
    LearnMore,
}

/// The dialog's `$state` while it is mounted.
pub struct OnboardingView {
    /// Bumped on every open: animations start over.
    pub mount: u64,
    pub step: Step,
    /// `activeFeatureIdx`.
    pub feature: usize,
    pub submitting: bool,
    pub rejecting: bool,
    /// `fadingOut`.
    pub fading: bool,
    pub persist_error: bool,
    /// `pendingConsent`.
    pending: Option<(bool, bool)>,
    /// When the "enough" row was pressed (`now` seconds): the two timers.
    reject_at: Option<f64>,
    save: Option<Pending<BackendResult<()>>>,
    /// `gifSize`: the clip's box when "enough" was pressed.
    pub gif_size: Option<Vec2>,
    /// `{#key rejecting}`: bumped each time `rejecting` flips.
    pub clip_mount: u64,
    /// Bumped on every step change: `{#if step}` mounts a new `.step`.
    pub step_mount: u64,
    /// When the deal step was entered: its title's colour animation.
    pub deal_since: f64,
}

impl OnboardingView {
    fn new(mount: u64) -> Self {
        Self {
            mount,
            step: Step::Welcome,
            feature: 0,
            submitting: false,
            rejecting: false,
            fading: false,
            persist_error: false,
            pending: None,
            reject_at: None,
            save: None,
            gif_size: None,
            clip_mount: 0,
            step_mount: 0,
            deal_since: 0.0,
        }
    }

    fn set_step(&mut self, step: Step, now: f64) {
        if step != self.step {
            self.step = step;
            self.step_mount += 1;
            if step == Step::Deal {
                self.deal_since = now;
            }
        }
    }

    fn set_rejecting(&mut self, rejecting: bool) {
        if rejecting != self.rejecting {
            self.rejecting = rejecting;
            self.clip_mount += 1;
        }
    }

    /// `finish(modeA, modeB)`: the save starts on the pool.
    fn finish(&mut self, jobs: &Jobs, mode_a: bool, mode_b: bool) {
        if self.submitting {
            return;
        }
        self.submitting = true;
        self.persist_error = false;
        self.pending = Some((mode_a, mode_b));
        self.save = Some(jobs.run(move |backend: &dyn Backend| {
            backend.telemetry_complete_onboarding(mode_a, mode_b)
        }));
    }
}

/// What a frame of the dialog asks of the app.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing.
    None,
    /// `onComplete`: the choice is saved, or put off.
    Complete,
}

/// The tour and the mounted dialog.
#[derive(Default)]
pub struct Onboarding {
    pub tour: OnboardingTour,
    pub view: Option<OnboardingView>,
    mounts: u64,
}

impl Onboarding {
    /// `openIfNeverCompleted`, started.
    pub fn start(&mut self, jobs: &Jobs) {
        self.tour.start_open_if_never_completed(jobs);
    }

    /// `openOnboarding` (the Privacy tab's replay).
    pub fn open(&mut self) {
        self.tour.open_onboarding();
    }

    /// The tour read, the timers, the save: once per frame. `active_tab`
    /// for the tour's tab swap; effects go to `apply_effects`.
    pub fn tick(&mut self, jobs: &Jobs, now: f64, active_tab: &str) -> (Outcome, Vec<AppEffect>) {
        self.tour.poll();
        let mut effects = Vec::new();
        if !self.tour.open() {
            self.view = None;
            return (Outcome::None, effects);
        }
        let view = self.view.get_or_insert_with(|| {
            self.mounts += 1;
            OnboardingView::new(self.mounts)
        });
        if let Some(at) = view.reject_at {
            if now - at >= REJECT_FADE_DELAY {
                view.fading = true;
            }
            if now - at >= REJECT_TOTAL {
                view.reject_at = None;
                view.finish(jobs, true, false);
            }
        }
        let mut outcome = Outcome::None;
        if let Some(save) = view.save.as_mut() {
            let saved = match save.poll() {
                Poll::Waiting => None,
                Poll::Ready(result) => Some(result.is_ok()),
                Poll::Gone => Some(false),
            };
            if let Some(saved) = saved {
                view.save = None;
                view.submitting = false;
                if saved {
                    outcome = Outcome::Complete;
                } else {
                    // The backdrop covers the toast host: the failure stays
                    // in the dialog, and the faded modal comes back.
                    view.persist_error = true;
                    view.fading = false;
                    view.set_rejecting(false);
                }
            }
        }
        if outcome == Outcome::Complete {
            effects.extend(self.close(active_tab));
        }
        (outcome, effects)
    }

    /// `onComplete`: `onboarding.close()`.
    pub fn close(&mut self, active_tab: &str) -> Vec<AppEffect> {
        self.view = None;
        self.tour.close(active_tab)
    }

    /// Applies one event; the returned effects swap the tab for the tour.
    pub fn apply(
        &mut self,
        jobs: &Jobs,
        event: OnboardingEvent,
        now: f64,
        active_tab: &str,
    ) -> Vec<AppEffect> {
        let Some(view) = self.view.as_mut() else {
            return Vec::new();
        };
        let was_features = view.step == Step::Features;
        match event {
            OnboardingEvent::Skip => view.finish(jobs, true, false),
            OnboardingEvent::GoFeatures => view.set_step(Step::Features, now),
            OnboardingEvent::PrevFeature => {
                if view.feature > 0 {
                    view.feature -= 1;
                } else {
                    view.set_step(Step::Welcome, now);
                }
            }
            OnboardingEvent::NextFeature => {
                if view.feature + 1 < FEATURE_COUNT {
                    view.feature += 1;
                } else {
                    view.set_step(Step::Deal, now);
                }
            }
            OnboardingEvent::Feature(index) => view.feature = index.min(FEATURE_COUNT - 1),
            OnboardingEvent::Enough(gif) => {
                if view.submitting || view.rejecting {
                    return Vec::new();
                }
                if let Some(size) = gif.filter(|s| s.x > 0.0 && s.y > 0.0) {
                    view.gif_size = Some(size);
                }
                view.set_rejecting(true);
                view.reject_at = Some(now);
            }
            OnboardingEvent::Deal => view.finish(jobs, true, true),
            OnboardingEvent::Refuse => view.finish(jobs, false, false),
            OnboardingEvent::Retry => {
                if let Some((a, b)) = view.pending {
                    view.finish(jobs, a, b);
                }
            }
            OnboardingEvent::Later => return self.close(active_tab),
            OnboardingEvent::LearnMore => {
                drop(jobs.run(|backend: &dyn Backend| backend.open_url(TELEMETRY_DOC_URL)));
            }
        }
        // `$effect(() => onTourActive(step === "features"))`.
        let features = view.step == Step::Features;
        if features != was_features {
            return self.tour.set_mock_active(features, active_tab);
        }
        Vec::new()
    }

    /// `hideActions`: the title bar keeps its buttons only during the tour.
    pub fn hide_actions(&self) -> bool {
        self.tour.open() && !self.tour.mock_active()
    }

    /// `onboarding.mockAccounts` while the tour's grid is up, the names
    /// translated.
    pub fn mock_grid(&self, locale: Locale) -> Option<Vec<PlatformAccount>> {
        self.tour.mock_active().then(|| {
            mock_accounts()
                .into_iter()
                .map(|mock| PlatformAccount {
                    display_name: mock.display_name.resolve(locale),
                    ..mock.account
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use accshift_app::mock::MockBackend;

    use super::*;

    /// The demo scenario, the onboarding never completed: its mock refuses
    /// `telemetry_complete_onboarding`, as the parity scenes rely on.
    fn jobs() -> Jobs {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(root.join("parity/data/demo.json")).unwrap();
        let mock = MockBackend::from_json(&text, root.parent().and_then(Path::parent)).unwrap();
        mock.set_onboarding(true);
        Jobs::new(Arc::new(mock), 1, || {})
    }

    /// Ticks at `now` until `done` holds, the pool answering in between.
    fn tick_until(
        o: &mut Onboarding,
        jobs: &Jobs,
        now: f64,
        done: impl Fn(&Onboarding) -> bool,
    ) -> Vec<AppEffect> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut effects = Vec::new();
        while !done(o) {
            assert!(Instant::now() < deadline, "timed out");
            effects.extend(o.tick(jobs, now, "steam").1);
            std::thread::sleep(Duration::from_millis(2));
        }
        effects
    }

    #[test]
    fn the_tour_swaps_the_tab_and_the_grid_while_on_the_features_step() {
        let jobs = jobs();
        let mut o = Onboarding::default();
        o.start(&jobs);
        tick_until(&mut o, &jobs, 0.0, |o| o.view.is_some());
        assert!(o.hide_actions());
        assert!(o.mock_grid(Locale::En).is_none());
        // From another tab, the tour moves to Steam and back.
        let effects = o.apply(&jobs, OnboardingEvent::GoFeatures, 0.1, "riot");
        assert_eq!(effects, vec![AppEffect::SetActiveTab("steam".into())]);
        assert!(!o.hide_actions());
        let grid = o.mock_grid(Locale::En).unwrap();
        assert!(!grid.is_empty());
        for _ in 0..FEATURE_COUNT - 1 {
            assert!(o
                .apply(&jobs, OnboardingEvent::NextFeature, 0.2, "steam")
                .is_empty());
        }
        let view = o.view.as_ref().unwrap();
        assert_eq!(
            (view.step, view.feature),
            (Step::Features, FEATURE_COUNT - 1)
        );
        let effects = o.apply(&jobs, OnboardingEvent::NextFeature, 0.3, "steam");
        assert_eq!(effects, vec![AppEffect::SetActiveTab("riot".into())]);
        let view = o.view.as_ref().unwrap();
        assert_eq!(view.step, Step::Deal);
        assert_eq!(view.deal_since, 0.3);
        assert!(o.mock_grid(Locale::En).is_none());
    }

    #[test]
    fn enough_fades_then_saves_and_a_refused_save_brings_the_dialog_back() {
        let jobs = jobs();
        let mut o = Onboarding::default();
        o.start(&jobs);
        tick_until(&mut o, &jobs, 0.0, |o| o.view.is_some());
        o.apply(&jobs, OnboardingEvent::GoFeatures, 0.0, "steam");
        for _ in 0..FEATURE_COUNT {
            o.apply(&jobs, OnboardingEvent::NextFeature, 0.0, "steam");
        }
        let clip = o.view.as_ref().unwrap().clip_mount;
        o.apply(
            &jobs,
            OnboardingEvent::Enough(Some(Vec2::new(88.56, 96.22))),
            10.0,
            "steam",
        );
        let view = o.view.as_ref().unwrap();
        assert!(view.rejecting && !view.fading);
        assert_eq!(view.clip_mount, clip + 1);
        assert_eq!(view.gif_size, Some(Vec2::new(88.56, 96.22)));
        // A second press does nothing while rejecting.
        o.apply(&jobs, OnboardingEvent::Enough(None), 10.5, "steam");
        assert_eq!(o.view.as_ref().unwrap().clip_mount, clip + 1);
        o.tick(&jobs, 11.1, "steam");
        assert!(!o.view.as_ref().unwrap().fading);
        o.tick(&jobs, 11.25, "steam");
        let view = o.view.as_ref().unwrap();
        assert!(view.fading && !view.submitting);
        o.tick(&jobs, 12.05, "steam");
        assert!(o.view.as_ref().unwrap().submitting);
        tick_until(&mut o, &jobs, 12.1, |o| {
            o.view.as_ref().is_some_and(|v| !v.submitting)
        });
        let view = o.view.as_ref().unwrap();
        assert!(view.persist_error && !view.fading && !view.rejecting);
        assert_eq!(view.clip_mount, clip + 2);
        // Retry tries the same choice again; Later closes without one.
        o.apply(&jobs, OnboardingEvent::Retry, 13.0, "steam");
        assert!(o.view.as_ref().unwrap().submitting);
        tick_until(&mut o, &jobs, 13.1, |o| {
            o.view.as_ref().is_some_and(|v| !v.submitting)
        });
        assert!(o.view.as_ref().unwrap().persist_error);
        o.apply(&jobs, OnboardingEvent::Later, 14.0, "steam");
        assert!(o.view.is_none() && !o.tour.open());
    }
}
