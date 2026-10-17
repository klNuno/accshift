//! The secure screen: the AFK cover and the PIN lock, ported from
//! `src/lib/app/useSecureScreen.svelte.ts` with the logic
//! `src/lib/app/AppScreenOverlays.svelte` and `src/App.svelte` keep inline.
//!
//! The Svelte `$effect`s become [`SecureScreen::update`], run once per frame
//! after the window and settings inputs are known: it applies finished
//! unlocks, fires due timers and replays the effects in source order. Timers
//! are deadlines ([`SecureScreen::next_deadline`]); the backend calls run on
//! [`Jobs`]. What the UI must do in return comes back as [`SecureEvent`]s.

use super::inactivity::InactivityBlur;
use super::pin::{
    is_pin_locked_error, is_valid_pin_hash, lock_pin_session, sanitize_pin_digits,
    unlock_pin_session, PinCheck, PIN_CODE_LENGTH,
};
use super::window_activity::WindowActivity;
use crate::error::BackendError;
use crate::jobs::{Jobs, Pending, Poll};
use crate::settings::AppSettings;
use crate::stores::ClientStores;
use crate::text::Text;
use std::time::{Duration, Instant};

/// `AFK_TEXT_FADE_MS` (line 57): how long the wave keeps running while the
/// AFK text fades out.
pub const AFK_TEXT_FADE: Duration = Duration::from_millis(900);

/// `AFK_TEXT_REVEAL_DELAY_MS` (line 58): the `startDelayMs` of the wave text.
pub const AFK_TEXT_REVEAL_DELAY_MS: u64 = 2500;

/// The pause between a matching code and the overlay going away (line 246).
pub const UNLOCK_SETTLE: Duration = Duration::from_millis(240);

/// What the screen reads from the window (`windowActivity` deps).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowView {
    pub foreground: bool,
    pub minimized: bool,
    pub page_visible: bool,
}

impl Default for WindowView {
    fn default() -> Self {
        Self::from(&WindowActivity::new())
    }
}

impl From<&WindowActivity> for WindowView {
    fn from(activity: &WindowActivity) -> Self {
        Self {
            foreground: activity.is_foreground(),
            minimized: activity.is_minimized(),
            page_visible: activity.is_page_visible(),
        }
    }
}

/// The per-frame inputs (`SecureScreenDeps` getters).
#[derive(Debug, Clone, Copy)]
pub struct SecureInputs<'a> {
    /// `getSettings()`: the shell's settings.
    pub settings: &'a AppSettings,
    /// `getIsAccountSelectionView()`: no settings panel and an adapter
    /// (App.svelte line 145). The AFK cover only covers the account grid;
    /// the PIN lock engages in every view.
    pub is_account_selection_view: bool,
    pub window: WindowView,
    /// `getAppVersion()`, empty until known.
    pub app_version: &'a str,
}

/// What the UI does in return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecureEvent {
    /// `onCloseContextMenu()`.
    CloseContextMenu,
    /// Put keyboard focus in the PIN field (the `setTimeout` focus calls and
    /// the AppScreenOverlays focus effect, lines 74-76).
    FocusPinInput,
    /// `persistPinHash(hash)`: a PBKDF2 hash replaced a legacy one. Write it
    /// with [`persist_pin_hash`].
    PersistPinHash(String),
}

#[derive(Debug, Clone, Default)]
struct Snapshot {
    pin_enabled: bool,
    pin_hash: String,
    inactivity_blur_seconds: i64,
    suspend_graphics_when_minimized: bool,
    is_account_selection_view: bool,
    window: WindowView,
    app_version: String,
}

impl Snapshot {
    fn read(&mut self, inputs: &SecureInputs<'_>) {
        let settings = inputs.settings;
        self.pin_enabled = settings.pin_enabled;
        if self.pin_hash != settings.pin_hash {
            self.pin_hash = settings.pin_hash.clone();
        }
        self.inactivity_blur_seconds = settings.inactivity_blur_seconds;
        self.suspend_graphics_when_minimized = settings.suspend_graphics_when_minimized;
        self.is_account_selection_view = inputs.is_account_selection_view;
        self.window = inputs.window;
        if self.app_version != inputs.app_version {
            self.app_version = inputs.app_version.to_string();
        }
    }

    fn has_valid_pin(&self) -> bool {
        is_valid_pin_hash(&self.pin_hash)
    }
}

/// `createSecureScreenController`.
pub struct SecureScreen {
    snap: Snapshot,
    blur: InactivityBlur,
    is_pin_locked: bool,
    is_pin_unlocking: bool,
    is_pin_retry_locked: bool,
    pin_attempt: String,
    pin_error: Option<Text>,
    retry_deadline: Option<Instant>,
    settle_deadline: Option<Instant>,
    unlock: Option<Pending<PinCheck>>,
    /// The backend refused a switch for want of the PIN. The screen stays
    /// locked on that alone, even when this window holds no PIN: only a code
    /// the backend accepts clears it (lines 82-85).
    backend_locked: bool,
    afk_listeners_attached: bool,
    afk_wave_active: bool,
    afk_wave_stop_deadline: Option<Instant>,
    prev_afk_visible: bool,
    prev_render_suspended: bool,
    prev_wants_focus: bool,
    events: Vec<SecureEvent>,
}

impl SecureScreen {
    /// Starts locked when the settings carry an enabled, well-formed PIN
    /// (`startupPinLocked`, lines 71-73).
    pub fn new(settings: &AppSettings, now: Instant) -> Self {
        let mut snap = Snapshot::default();
        snap.read(&SecureInputs {
            settings,
            is_account_selection_view: false,
            window: WindowView::default(),
            app_version: "",
        });
        let startup_locked = snap.pin_enabled && snap.has_valid_pin();
        Self {
            snap,
            blur: InactivityBlur::new(now),
            is_pin_locked: startup_locked,
            is_pin_unlocking: false,
            is_pin_retry_locked: false,
            pin_attempt: String::new(),
            pin_error: None,
            retry_deadline: None,
            settle_deadline: None,
            unlock: None,
            backend_locked: false,
            afk_listeners_attached: false,
            afk_wave_active: false,
            afk_wave_stop_deadline: None,
            prev_afk_visible: false,
            prev_render_suspended: false,
            prev_wants_focus: false,
            events: Vec::new(),
        }
    }

    // ------------------------------------------------------------ state

    pub fn blur(&self) -> &InactivityBlur {
        &self.blur
    }

    pub fn is_pin_locked(&self) -> bool {
        self.is_pin_locked
    }

    pub fn is_pin_unlocking(&self) -> bool {
        self.is_pin_unlocking
    }

    pub fn is_pin_retry_locked(&self) -> bool {
        self.is_pin_retry_locked
    }

    /// The digits typed so far. Draw it masked.
    pub fn pin_attempt(&self) -> &str {
        &self.pin_attempt
    }

    /// `pinError`: `pin.invalid` or `pin.tooManyAttempts { seconds }`.
    pub fn pin_error(&self) -> Option<&Text> {
        self.pin_error.as_ref()
    }

    /// `pinCodeLength`.
    pub fn pin_code_length(&self) -> usize {
        PIN_CODE_LENGTH
    }

    /// `afkTextRevealDelayMs`.
    pub fn afk_text_reveal_delay_ms(&self) -> u64 {
        AFK_TEXT_REVEAL_DELAY_MS
    }

    /// The backend said it is locked and no accepted code cleared it yet.
    pub fn is_backend_locked(&self) -> bool {
        self.backend_locked
    }

    /// `windowForeground`.
    pub fn window_foreground(&self) -> bool {
        self.snap.window.foreground
    }

    fn window_renderable(&self) -> bool {
        self.snap.window.page_visible && !self.snap.window.minimized
    }

    /// `renderSuspended`: minimized with "suspend graphics" on.
    pub fn render_suspended(&self) -> bool {
        self.snap.suspend_graphics_when_minimized && self.snap.window.minimized
    }

    fn inactivity_enabled(&self) -> bool {
        self.snap.inactivity_blur_seconds > 0
    }

    fn pin_overlay_up(&self) -> bool {
        self.is_pin_locked || self.is_pin_unlocking || self.is_pin_retry_locked
    }

    fn afk_covering(&self) -> bool {
        self.inactivity_enabled() && self.blur.is_blurred() && self.snap.is_account_selection_view
    }

    /// `isObscured` (lines 96-101): the frost over the app.
    pub fn is_obscured(&self) -> bool {
        self.afk_covering() || self.pin_overlay_up()
    }

    /// `afkOverlayVisible` (lines 102-111).
    pub fn afk_overlay_visible(&self) -> bool {
        self.afk_covering()
            && !self.pin_overlay_up()
            && self.window_renderable()
            && !self.render_suspended()
    }

    /// `motionPaused`.
    pub fn motion_paused(&self) -> bool {
        !self.window_renderable() || self.render_suspended()
    }

    /// `afkWaveActive`: the AFK wave text runs (it keeps running for
    /// [`AFK_TEXT_FADE`] after the cover goes).
    pub fn afk_wave_active(&self) -> bool {
        self.afk_wave_active
    }

    /// `afkVersionLabel`.
    pub fn afk_version_label(&self) -> Option<&str> {
        (self.afk_overlay_visible() && !self.snap.app_version.is_empty())
            .then_some(self.snap.app_version.as_str())
    }

    /// The PIN overlay is drawn (AppScreenOverlays lines 113 and 130).
    pub fn pin_overlay_visible(&self) -> bool {
        !self.render_suspended() && self.pin_overlay_up()
    }

    /// The PIN field takes input (AppScreenOverlays line 161).
    pub fn pin_input_enabled(&self) -> bool {
        !self.is_pin_unlocking && !self.is_pin_retry_locked
    }

    /// An unlock is waiting for the backend.
    pub fn is_busy(&self) -> bool {
        self.unlock.is_some()
    }

    /// The earliest timer, for the repaint request.
    pub fn next_deadline(&self) -> Option<Instant> {
        [
            self.retry_deadline,
            self.settle_deadline,
            self.afk_wave_stop_deadline,
            self.blur.next_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    // ------------------------------------------------------------ input

    /// `setPinAttempt`: keeps the digits only, four at most (the attempt
    /// effect, lines 197-207). The unlock starts on the next
    /// [`Self::update`].
    pub fn set_pin_attempt(&mut self, value: &str) {
        self.pin_attempt = sanitize_pin_digits(value);
    }

    /// A pointer move over the window (the blur's `mousemove` listener).
    pub fn on_pointer_move(&mut self, now: Instant) {
        self.blur
            .on_pointer_move(now, self.snap.inactivity_blur_seconds);
    }

    /// A mouse button press (`mousedown`).
    pub fn on_mouse_down(&mut self, now: Instant) {
        self.blur
            .on_mouse_down(now, self.snap.inactivity_blur_seconds);
    }

    /// A key press (`keydown`).
    pub fn on_key_down(&mut self, now: Instant) {
        self.blur
            .on_key_down(now, self.snap.inactivity_blur_seconds);
    }

    // ------------------------------------------------------------ lifecycle

    /// `handleAppMounted` (lines 261-276). Call once after the first
    /// [`Self::update`] fed the real settings.
    pub fn handle_app_mounted(&mut self, jobs: &Jobs, now: Instant) {
        self.blur.start(now, self.snap.inactivity_blur_seconds);
        self.blur.attach_listeners();
        self.afk_listeners_attached = true;
        if self.is_pin_locked {
            // The backend locks itself at start when the store holds a PIN.
            // Say it anyway: this may be a reload of an unlocked session.
            tell_backend_lock(jobs);
            self.is_pin_retry_locked = false;
            self.pin_attempt.clear();
            self.pin_error = None;
            self.events.push(SecureEvent::FocusPinInput);
        }
    }

    /// `handleAppDestroyed` (lines 278-293).
    pub fn handle_app_destroyed(&mut self) {
        self.afk_wave_stop_deadline = None;
        self.retry_deadline = None;
        if self.afk_listeners_attached {
            self.blur.detach_listeners();
        }
        self.blur.stop();
    }

    /// `handleSettingsClosed` (lines 253-259): the timer starts over.
    pub fn handle_settings_closed(&mut self, now: Instant) {
        self.blur.start(now, self.snap.inactivity_blur_seconds);
        if !self.afk_listeners_attached {
            self.blur.attach_listeners();
            self.afk_listeners_attached = true;
        }
    }

    /// `handleBackendLocked` (lines 190-195): the backend refused a command
    /// for want of the PIN.
    pub fn handle_backend_locked(&mut self) {
        self.backend_locked = true;
        if self.is_pin_locked || self.is_pin_unlocking {
            return;
        }
        // The backend is locked already: no need to tell it.
        self.engage_lock(None);
    }

    /// `reportPinLockedError` (pinSession.ts lines 33-37) wired to this
    /// screen: true when `error` was the PIN refusal, so the caller skips
    /// its generic failure message.
    pub fn report_backend_error(&mut self, error: &BackendError) -> bool {
        if !is_pin_locked_error(error) {
            return false;
        }
        self.handle_backend_locked();
        true
    }

    // ------------------------------------------------------------ frame

    /// One frame: read the inputs, apply a finished unlock, fire due timers,
    /// then run the effects of lines 115-207 in order.
    pub fn update(
        &mut self,
        inputs: &SecureInputs<'_>,
        jobs: &Jobs,
        now: Instant,
    ) -> Vec<SecureEvent> {
        self.snap.read(inputs);
        self.poll_unlock(now);
        self.fire_timers(now);

        // AFK wave (lines 115-131).
        let afk_visible = self.afk_overlay_visible();
        if afk_visible && !self.prev_afk_visible {
            self.afk_wave_stop_deadline = None;
            self.events.push(SecureEvent::CloseContextMenu);
            self.afk_wave_active = true;
        } else if !afk_visible && self.prev_afk_visible && self.afk_wave_active {
            self.afk_wave_stop_deadline = Some(now + AFK_TEXT_FADE);
        }
        self.prev_afk_visible = afk_visible;

        // Render suspend closes the context menu (lines 133-137).
        let suspended = self.render_suspended();
        if suspended && !self.prev_render_suspended {
            self.events.push(SecureEvent::CloseContextMenu);
        }
        self.prev_render_suspended = suspended;

        // Listeners follow render suspend (lines 139-151).
        if suspended {
            if self.afk_listeners_attached {
                self.blur.detach_listeners();
                self.afk_listeners_attached = false;
            }
        } else if !self.afk_listeners_attached {
            self.blur.attach_listeners();
            self.afk_listeners_attached = true;
        }

        // No PIN any more: drop the lock (lines 153-162).
        if (!self.snap.pin_enabled || !self.snap.has_valid_pin()) && !self.backend_locked {
            self.is_pin_locked = false;
            self.is_pin_retry_locked = false;
            self.pin_attempt.clear();
            self.pin_error = None;
        }

        // Inactivity engages the PIN lock (lines 164-177).
        if self.blur.is_blurred()
            && self.snap.pin_enabled
            && self.snap.has_valid_pin()
            && !self.is_pin_locked
            && !self.is_pin_unlocking
        {
            self.engage_lock(Some(jobs));
        }

        // Four digits typed: unlock (lines 197-207).
        if self.is_pin_locked
            && !self.is_pin_unlocking
            && !self.is_pin_retry_locked
            && self.pin_attempt.len() == PIN_CODE_LENGTH
        {
            self.start_unlock(jobs);
        }

        // The field holds focus while the lock waits for a code
        // (AppScreenOverlays lines 74-76).
        let wants_focus = self.is_pin_locked && !self.is_pin_unlocking;
        if wants_focus && !self.prev_wants_focus {
            self.events.push(SecureEvent::FocusPinInput);
        }
        self.prev_wants_focus = wants_focus;

        let mut events = std::mem::take(&mut self.events);
        let mut seen = Vec::with_capacity(events.len());
        events.retain(|event| {
            if seen.contains(event) {
                return false;
            }
            seen.push(event.clone());
            true
        });
        events
    }

    fn fire_timers(&mut self, now: Instant) {
        if self.retry_deadline.is_some_and(|due| now >= due) {
            self.retry_deadline = None;
            self.is_pin_retry_locked = false;
            self.events.push(SecureEvent::FocusPinInput);
        }
        if self.settle_deadline.is_some_and(|due| now >= due) {
            self.settle_deadline = None;
            self.is_pin_locked = false;
            self.is_pin_unlocking = false;
            self.blur
                .reset_activity(now, self.snap.inactivity_blur_seconds);
        }
        if self.afk_wave_stop_deadline.is_some_and(|due| now >= due) {
            self.afk_wave_stop_deadline = None;
            self.afk_wave_active = false;
        }
        self.blur.tick(now, self.snap.inactivity_blur_seconds);
    }

    /// `engageLock` (lines 180-188). `None` skips telling the backend.
    fn engage_lock(&mut self, tell_backend: Option<&Jobs>) {
        if let Some(jobs) = tell_backend {
            tell_backend_lock(jobs);
        }
        self.is_pin_locked = true;
        self.is_pin_retry_locked = false;
        self.pin_attempt.clear();
        self.pin_error = None;
        self.events.push(SecureEvent::CloseContextMenu);
        self.events.push(SecureEvent::FocusPinInput);
    }

    /// The first half of `unlockWithPin` (lines 209-221).
    fn start_unlock(&mut self, jobs: &Jobs) {
        let expected = self.snap.pin_hash.clone();
        if !is_valid_pin_hash(&expected) && !self.backend_locked {
            self.is_pin_locked = false;
            return;
        }
        let attempt = sanitize_pin_digits(&self.pin_attempt);
        if attempt.len() != PIN_CODE_LENGTH || self.is_pin_retry_locked {
            return;
        }
        self.is_pin_unlocking = true;
        self.pin_error = None;
        // The backend checks the code and unlocks the switch commands. It
        // also rate limits wrong codes, so its wait wins over the local one.
        self.unlock =
            Some(jobs.run(move |backend| unlock_pin_session(backend, &attempt, &expected)));
    }

    fn poll_unlock(&mut self, now: Instant) {
        let Some(pending) = self.unlock.as_mut() else {
            return;
        };
        let check = match pending.poll() {
            Poll::Waiting => return,
            Poll::Ready(check) => check,
            // The check panicked: a plain wrong code, so the screen never
            // hangs in "unlocking".
            Poll::Gone => PinCheck::Invalid { retry_after_ms: 0 },
        };
        self.unlock = None;
        self.apply_unlock(check, now);
    }

    /// The second half of `unlockWithPin` (lines 222-251).
    fn apply_unlock(&mut self, check: PinCheck, now: Instant) {
        if let Some(wait) = check.failure_wait() {
            self.is_pin_unlocking = false;
            self.is_pin_retry_locked = true;
            self.pin_error = Some(if wait.too_many_attempts {
                Text::with(
                    "pin.tooManyAttempts",
                    [("seconds", wait.wait_ms.div_ceil(1000))],
                )
            } else {
                Text::key("pin.invalid")
            });
            self.pin_attempt.clear();
            self.retry_deadline = Some(now + Duration::from_millis(wait.wait_ms));
            return;
        }
        self.backend_locked = false;
        if let PinCheck::Match {
            rehashed: Some(hash),
        } = check
        {
            // Unlocked against the old unsalted hash: store the PBKDF2 one
            // now, so the next unlock (here or in the CLI) runs salted.
            self.events.push(SecureEvent::PersistPinHash(hash));
        }
        self.pin_attempt.clear();
        self.settle_deadline = Some(now + UNLOCK_SETTLE);
    }
}

fn tell_backend_lock(jobs: &Jobs) {
    // Fire and forget, as `lockPinSession`: the result is dropped.
    drop(jobs.run(lock_pin_session));
}

/// `persistPinHash` (App.svelte lines 256-261): re-read the store rather
/// than write the shell's settings back, since the settings panel may hold a
/// draft; only the hash travels.
pub fn persist_pin_hash(stores: &mut ClientStores, hash: &str) {
    let mut latest = stores.settings().clone();
    latest.pin_hash = hash.to_string();
    stores.save_settings(&latest);
}
