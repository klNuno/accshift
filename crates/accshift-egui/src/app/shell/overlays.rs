//! What draws over the stage: the onboarding, then the screen overlays
//! (the AFK cover, the streamer overlay and the PIN lock).

use egui::{Rect, Ui, UiBuilder};

use super::super::onboarding::OnboardingView;
use super::super::platforms::{PlatformDef, RuntimeOs};
use super::super::{Action, Booted};
use super::FrameView;
use crate::overlay_output::OverlayA11y;
use crate::ui::components::onboarding::{self, OnboardingProps};
use crate::ui::components::secure_overlays::{self, AfkProps, PinProps, StreamerProps};

/// What the AFK cover draws this frame.
pub(super) struct AfkView {
    pub(super) visible: bool,
    wave: bool,
    reveal_delay: f64,
    pub(super) version: Option<String>,
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

/// Runs the unscaled overlay pass (onboarding, AFK cover, streamer mode,
/// PIN lock) at 100% UI scale on `overlay_ctx`, regardless of `uiScalePercent`.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_overlay_pass(
    overlay_ctx: &egui::Context,
    overlay_fonts_cjk: &mut Option<bool>,
    overlay_output: &mut Option<egui::FullOutput>,
    overlay_active: &mut bool,
    ctx: &egui::Context,
    fv: &FrameView,
    stage_top: f32,
    tour: Option<(&OnboardingView, &[String], bool)>,
    secure: SecureView,
    pin_focus: bool,
    a11y: &mut OverlayA11y,
) -> Vec<Action> {
    let ui_zoom = crate::zoom::frame(ctx);
    let window_css = crate::zoom::viewport(ctx);
    let overlay_screen = Rect::from_min_size(egui::Pos2::ZERO, window_css);
    // Screen overlays sit outside the stage: the visible window, under a
    // macOS title bar (which lives in stage points).
    let overlay_rect =
        Rect::from_min_max(egui::pos2(0.0, stage_top * ui_zoom), window_css.to_pos2());

    overlay_ctx.options_mut(|o| {
        o.input_options.surrender_focus_on = egui::SurrenderFocusOn::Presses;
        o.zoom_with_keyboard = false;
    });
    let zh = fv.locale == accshift_app::i18n::Locale::Zh;
    if *overlay_fonts_cjk != Some(zh) {
        crate::ui::text::install(overlay_ctx, zh);
        *overlay_fonts_cjk = Some(zh);
    }
    crate::ui::anim::set_motion(overlay_ctx, fv.motion);
    let base = crate::zoom::base(ctx);
    if (overlay_ctx.zoom_factor() - base).abs() > 1e-5 {
        overlay_ctx.set_zoom_factor(base);
    }
    onboarding::set_targets(overlay_ctx, onboarding::targets(ctx).scale(ui_zoom));
    // The overlay's AccessKit tree joins the window's while a screen reader
    // listens (the stage's pass then has a root node).
    if ctx.has_accesskit_node(egui::accesskit_root_id()) {
        overlay_ctx.enable_accesskit();
    } else {
        overlay_ctx.disable_accesskit();
    }

    // The events are in the points of the zoom factor in force when they
    // arrived, not yet the one this pass applies.
    let input_zoom = crate::zoom::input_frame(ctx);
    let (focused, viewports, events) = ctx.input(|i| {
        // The window's modifiers first: the overlay only sees the events of
        // the frames it runs, and they may have changed in between.
        let modifiers = egui::Event::ModifiersChanged(i.modifiers);
        let events = std::iter::once(modifiers)
            .chain(crate::zoom::scale_events(&i.events, input_zoom))
            .collect();
        (
            i.focused,
            i.raw.viewports.clone(),
            a11y.overlay_events(events),
        )
    });
    let overlay_raw = egui::RawInput {
        screen_rect: Some(overlay_screen),
        time: Some(fv.now),
        focused,
        viewports,
        events,
        ..Default::default()
    };

    let mut actions = Vec::new();
    let mut overlay_out = overlay_ctx.run_ui(overlay_raw, |overlay_ui| {
        crate::ui::text::set_transform(overlay_ui.ctx(), None);
        crate::ui::components::track_held(overlay_ui.ctx());
        if let Some((view, names, detected)) = tour {
            actions.extend(draw_tour(
                overlay_ui,
                fv,
                overlay_screen,
                fv.title_bar_h * ui_zoom,
                view,
                names,
                detected,
            ));
        }
        actions.extend(draw_screen_overlays(
            overlay_ui,
            fv,
            overlay_rect,
            &secure,
            pin_focus,
        ));
    });

    if let Some(vp) = overlay_out.viewport_output.get(&egui::ViewportId::ROOT) {
        if vp.repaint_delay.is_zero() {
            ctx.request_repaint();
        } else if vp.repaint_delay < std::time::Duration::from_secs(3600) {
            ctx.request_repaint_after(vp.repaint_delay);
        }
        for cmd in &vp.commands {
            ctx.send_viewport_cmd(cmd.clone());
        }
    }

    let tree = overlay_out.platform_output.accesskit_update.take();
    let scale = overlay_out.pixels_per_point / ctx.pixels_per_point();
    overlay_out.platform_output.accesskit_update = a11y.subtree(tree, scale);

    *overlay_active = !overlay_out.shapes.is_empty();
    if *overlay_active || !overlay_out.textures_delta.is_empty() {
        *overlay_output = Some(overlay_out);
    } else {
        overlay_out.textures_delta.clear();
        *overlay_output = None;
        a11y.clear();
    }
    actions
}

/// The telemetry onboarding: over the frame, out of the obscured filter,
/// under the screen overlays.
fn draw_tour(
    ui: &mut Ui,
    fv: &FrameView,
    viewport: Rect,
    titlebar_h: f32,
    view: &OnboardingView,
    names: &[String],
    detected: bool,
) -> Vec<Action> {
    let props = OnboardingProps {
        vars: &fv.vars,
        locale: fv.locale,
        viewport,
        titlebar_h,
        version: accshift_app::lifecycle::updater::APP_VERSION,
        platforms: names,
        detected,
        view,
        focus_visible: fv.focus_visible,
        dark: fv.dark,
        now: fv.now,
    };
    let mut frame_ui = ui.new_child(UiBuilder::new().max_rect(viewport));
    onboarding::onboarding(&mut frame_ui, &props)
        .into_iter()
        .map(Action::Onboarding)
        .collect()
}

/// The AFK cover, the streamer overlay, then the PIN lock.
fn draw_screen_overlays(
    ui: &mut Ui,
    fv: &FrameView,
    overlay: Rect,
    secure: &SecureView,
    pin_focus: bool,
) -> Vec<Action> {
    let mut actions = Vec::new();
    if let Some(afk) = &secure.afk {
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
    if let Some(pin) = &secure.pin {
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
