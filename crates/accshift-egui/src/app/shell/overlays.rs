//! What draws over the stage: the onboarding, then the screen overlays
//! (the AFK cover, the streamer overlay and the PIN lock).

use egui::{Rect, Ui, UiBuilder};

use super::super::onboarding::OnboardingView;
use super::super::platforms::{PlatformDef, RuntimeOs};
use super::super::{Action, Booted};
use super::FrameView;
use crate::ui::components::onboarding::{self, OnboardingProps};
use crate::ui::components::secure_overlays::{self, AfkProps, PinProps, StreamerProps};

/// What the AFK cover draws this frame.
pub(super) struct AfkView {
    visible: bool,
    wave: bool,
    reveal_delay: f64,
    version: Option<String>,
}

/// What the streamer overlay draws this frame.
pub(super) struct StreamerView {
    pub(super) active: bool,
    /// The overlay's motion is not paused.
    pub(super) motion: bool,
}

/// What the PIN lock draws this frame.
pub(super) struct PinView {
    attempt: String,
    error: Option<String>,
    input_enabled: bool,
    unlocking: bool,
    code_len: usize,
}

/// The secure screen and streamer mode this frame.
pub(super) struct SecureView {
    /// The shell is blurred under the secure screen.
    pub(super) obscured: bool,
    pub(super) afk: Option<AfkView>,
    pub(super) pin: Option<PinView>,
    pub(super) streamer: StreamerView,
}

impl SecureView {
    pub(super) fn read(b: &Booted) -> SecureView {
        let s = &b.secure;
        let afk = (!s.render_suspended()).then(|| AfkView {
            visible: s.afk_overlay_visible(),
            wave: s.afk_wave_active() && !s.motion_paused(),
            reveal_delay: s.afk_text_reveal_delay_ms() as f64 / 1000.0,
            version: s.afk_version_label().map(str::to_string),
        });
        let pin = s.pin_overlay_visible().then(|| PinView {
            attempt: s.pin_attempt().to_string(),
            error: s.pin_error().map(|t| t.resolve(b.locale)),
            input_enabled: s.pin_input_enabled(),
            unlocking: s.is_pin_unlocking(),
            code_len: s.pin_code_length(),
        });
        let streamer = StreamerView {
            active: b.streamer.active(),
            motion: !s.motion_paused(),
        };
        SecureView {
            obscured: s.is_obscured(),
            afk,
            pin,
            streamer,
        }
    }
}

/// The platform names the onboarding lists: the detected ones in registry
/// order when detection found something, else every platform this OS runs.
pub(super) fn tour_platforms(
    platforms: &[PlatformDef],
    detected: &[String],
    os: RuntimeOs,
) -> Vec<String> {
    platforms
        .iter()
        .filter(|p| {
            if detected.is_empty() {
                p.supports(os)
            } else {
                detected.contains(&p.id)
            }
        })
        .map(|p| p.name.clone())
        .collect()
}

/// The telemetry onboarding: over the frame, out of the obscured filter,
/// under the screen overlays.
pub(super) fn draw_tour(
    ui: &mut Ui,
    fv: &FrameView,
    view: &OnboardingView,
    names: &[String],
    detected: bool,
) -> Vec<Action> {
    let props = OnboardingProps {
        vars: &fv.vars,
        locale: fv.locale,
        viewport: fv.screen,
        titlebar_h: fv.title_bar_h,
        version: accshift_app::lifecycle::updater::APP_VERSION,
        platforms: names,
        detected,
        view,
        focus_visible: fv.focus_visible,
        dark: fv.dark,
        now: fv.now,
    };
    let mut frame_ui = ui.new_child(UiBuilder::new().max_rect(fv.screen));
    onboarding::onboarding(&mut frame_ui, &props)
        .into_iter()
        .map(Action::Onboarding)
        .collect()
}

/// The AFK cover, the streamer overlay, then the PIN lock.
pub(super) fn draw_screen_overlays(
    ui: &mut Ui,
    fv: &FrameView,
    overlay: Rect,
    secure: SecureView,
    pin_focus: bool,
) -> Vec<Action> {
    let mut actions = Vec::new();
    if let Some(afk) = secure.afk {
        secure_overlays::afk_overlay(
            ui,
            overlay,
            &AfkProps {
                vars: &fv.vars,
                visible: afk.visible,
                wave: afk.wave,
                reveal_delay: afk.reveal_delay,
                version: afk.version.as_deref(),
            },
        );
    }
    let props = StreamerProps {
        vars: &fv.vars,
        locale: fv.locale,
        active: secure.streamer.active,
        motion: secure.streamer.motion,
        focus_visible: fv.focus_visible,
    };
    actions.extend(
        secure_overlays::streamer_overlay(ui, overlay, &props)
            .into_iter()
            .map(Action::Streamer),
    );
    if let Some(pin) = secure.pin {
        let props = PinProps {
            vars: &fv.vars,
            locale: fv.locale,
            attempt: &pin.attempt,
            error: pin.error.as_deref(),
            input_enabled: pin.input_enabled,
            unlocking: pin.unlocking,
            focus: pin_focus,
            code_len: pin.code_len,
            dark: fv.dark,
        };
        actions.extend(
            secure_overlays::pin_overlay(ui, overlay, &props)
                .into_iter()
                .map(Action::Pin),
        );
    }
    actions
}
