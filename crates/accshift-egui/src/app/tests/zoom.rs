use super::*;
use crate::app::onboarding::Step;
use crate::{Screen, Shell};
use egui::{
    pos2, vec2, Event, FullOutput, Key, Modifiers, PointerButton, Pos2, RawInput, Rect,
    ViewportCommand, ViewportId,
};
use serde_json::json;

const PIN_HASH_1234: &str = "000102030405060708090a0b0c0d0e0f:e19d9507e40b77fbb7503faedce7cb4ebf8c6820a8b746d9dfa9fcab899ec65d";

/// Builds a booted `Shell` over the `demo` scenario with custom settings.
fn demo_shell(ctx: &Context, settings_patch: Value, streaming: bool, onboarding: bool) -> Shell {
    let mut fixture: Value =
        serde_json::from_str(include_str!("../../../parity/data/demo.json")).unwrap();
    fixture["spec"]["switchDelayMs"] = json!(0);
    let settings = &mut fixture["commands"]["get_boot_payload"][0]["result"]["storageSnapshot"]
        ["stores"]["client.settings"];
    if let (Some(dst), Some(src)) = (settings.as_object_mut(), settings_patch.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    let mock = Arc::new(MockBackend::from_json(&fixture.to_string(), None).unwrap());
    mock.set_streaming(streaming);
    mock.set_onboarding(onboarding);
    let clock = Clock::pinned(mock.now());
    let mut app = AppModel::with_backend(ctx, mock.clone(), Some(mock), clock);
    app.wait_boot(ctx).unwrap();
    Shell::new(Screen::App(Box::new(app)), None)
}

/// Runs one `Shell` frame at `window_size` CSS px (`scale = 1.0`) with
/// `events` in window CSS coordinates, matching `soft.rs`'s input/zoom path.
fn step_shell(
    shell: &mut Shell,
    ctx: &Context,
    window_size: [f32; 2],
    time: f64,
    events: Vec<Event>,
) -> FullOutput {
    let stage = shell.stage_zoom(ctx);
    let ppp = ctx.pixels_per_point();
    let mut raw = RawInput {
        focused: true,
        time: Some(time),
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            vec2(window_size[0] / ppp, window_size[1] / ppp),
        )),
        events: events
            .into_iter()
            .map(|e| match e {
                Event::PointerMoved(p) => Event::PointerMoved((p.to_vec2() / ppp).to_pos2()),
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => Event::PointerButton {
                    pos: (pos.to_vec2() / ppp).to_pos2(),
                    button,
                    pressed,
                    modifiers,
                },
                other => other,
            })
            .collect(),
        ..Default::default()
    };
    let vp = raw.viewports.get_mut(&ViewportId::ROOT).unwrap();
    vp.focused = Some(true);
    vp.minimized = Some(false);
    vp.native_pixels_per_point = Some(1.0);
    shell.input_hook(ctx, &mut raw, stage);
    crate::zoom::begin_frame(ctx, stage);
    let mut out = ctx.run_ui(raw, |ui| shell.frame(ui));
    out.textures_delta.clear();
    out
}

fn click_at(
    shell: &mut Shell,
    ctx: &Context,
    window_size: [f32; 2],
    time: &mut f64,
    pos: Pos2,
) -> Vec<ViewportCommand> {
    let mut cmds = Vec::new();
    for events in [
        vec![Event::PointerMoved(pos)],
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    ] {
        *time += 0.05;
        let out = step_shell(shell, ctx, window_size, *time, events);
        if let Some(vp) = out.viewport_output.get(&ViewportId::ROOT) {
            cmds.extend(vp.commands.iter().cloned());
        }
    }
    cmds
}

fn app_ref(shell: &Shell) -> &AppModel {
    match &shell.screen {
        Screen::App(app) => app,
        _ => panic!("expected Screen::App"),
    }
}

#[test]
fn pin_lock_caption_buttons_stay_unscaled_at_150_percent() {
    for animations in ["off", "on"] {
        let ctx = Context::default();
        let mut shell = demo_shell(
            &ctx,
            json!({
                "animations": animations,
                "uiScalePercent": 150,
                "pinEnabled": true,
                "pinHash": PIN_HASH_1234,
            }),
            false,
            false,
        );
        let size = [860.0, 440.0];
        let mut t = 1.0;
        for _ in 0..4 {
            t += 0.05;
            step_shell(&mut shell, &ctx, size, t, Vec::new());
        }
        assert!(
            app_ref(&shell)
                .booted()
                .unwrap()
                .secure
                .pin_overlay_visible(),
            "PIN lock visible ({animations})"
        );
        // At 150% scaled, the 36px close button stretches to [806..860, 0..54],
        // so (810, 45) wrongly hits it. At 100%, the button is [824..860, 0..36].
        let missed = click_at(&mut shell, &ctx, size, &mut t, pos2(810.0, 45.0));
        assert!(
            !missed.contains(&ViewportCommand::Close),
            "PIN close button must not stretch to (810, 45) at 150% ({animations})"
        );
        let hit = click_at(&mut shell, &ctx, size, &mut t, pos2(842.0, 18.0));
        assert!(
            hit.contains(&ViewportCommand::Close),
            "PIN close button must hit at 100% position (842, 18) ({animations})"
        );
    }
}

#[test]
fn streamer_overlay_buttons_stay_unscaled_at_150_percent() {
    for animations in ["off", "on"] {
        let ctx = Context::default();
        let mut shell = demo_shell(
            &ctx,
            json!({
                "animations": animations,
                "uiScalePercent": 150,
                "streamerMode": "auto",
            }),
            true,
            false,
        );
        let size = [860.0, 440.0];
        let mut t = 1.0;
        for _ in 0..5 {
            t += 0.4;
            step_shell(&mut shell, &ctx, size, t, Vec::new());
        }
        assert!(
            app_ref(&shell).booted().unwrap().streamer.active(),
            "streamer overlay active ({animations})"
        );
        // Caption close button stays 36x36 at 100%, not 54x54 at 150%.
        let missed = click_at(&mut shell, &ctx, size, &mut t, pos2(810.0, 45.0));
        assert!(
            !missed.contains(&ViewportCommand::Close),
            "streamer close button must not stretch to (810, 45) at 150% ({animations})"
        );
        // Exact 100% coordinates of the "Disable" button in streamer-glass-dark-100.
        click_at(&mut shell, &ctx, size, &mut t, pos2(351.0, 268.0));
        t += 0.05;
        step_shell(&mut shell, &ctx, size, t, Vec::new());
        assert!(
            !app_ref(&shell).booted().unwrap().streamer.active(),
            "streamer Disable button must stay at 100% coordinates (351, 268) at 150% ({animations})"
        );
    }
}

#[test]
fn afk_overlay_stays_unscaled_at_150_percent() {
    for animations in ["off", "on"] {
        let bounds_at = |scale_percent: i64| -> (f32, Rect) {
            let ctx = Context::default();
            let mut shell = demo_shell(
                &ctx,
                json!({
                    "animations": animations,
                    "uiScalePercent": scale_percent,
                    "inactivityBlurSeconds": 1,
                }),
                false,
                false,
            );
            let size = [860.0, 440.0];
            let mut t = 1.0;
            // Let boot settle, then trigger AFK after >1s inactivity.
            for _ in 0..3 {
                t += 0.05;
                step_shell(&mut shell, &ctx, size, t, Vec::new());
            }
            std::thread::sleep(std::time::Duration::from_millis(1100));
            for _ in 0..4 {
                t += 0.5;
                step_shell(&mut shell, &ctx, size, t, Vec::new());
            }
            assert!(
                app_ref(&shell)
                    .booted()
                    .unwrap()
                    .secure
                    .afk_overlay_visible(),
                "AFK overlay visible at {scale_percent}% ({animations})"
            );
            let (_, mut overlay_out) = shell
                .take_overlay()
                .expect("AFK overlay must render on the unscaled overlay pass");
            overlay_out.textures_delta.clear();
            let ppp = overlay_out.pixels_per_point;
            let mut union = Rect::NOTHING;
            for cs in &overlay_out.shapes {
                let r = cs.shape.visual_bounding_rect();
                if r.is_positive() {
                    union = union.union(Rect::from_min_max(
                        (r.min.to_vec2() * ppp).to_pos2(),
                        (r.max.to_vec2() * ppp).to_pos2(),
                    ));
                }
            }
            (ppp, union)
        };
        let (ppp_100, rect_100) = bounds_at(100);
        let (ppp_150, rect_150) = bounds_at(150);
        assert!(
            (ppp_100 - 1.0).abs() < 1e-5 && (ppp_150 - 1.0).abs() < 1e-5,
            "AFK overlay ppp must stay 1.0 (got {ppp_100} vs {ppp_150}, {animations})"
        );
        assert!(
            rect_100.is_positive() && (rect_100.width() - rect_150.width()).abs() < 0.5
                && (rect_100.height() - rect_150.height()).abs() < 0.5
                && (rect_100.min.x - rect_150.min.x).abs() < 0.5
                && (rect_100.min.y - rect_150.min.y).abs() < 0.5,
            "AFK overlay physical bounds must match at 100% ({rect_100:?}) and 150% ({rect_150:?}) ({animations})"
        );
    }
}

#[test]
fn onboarding_modal_stays_unscaled_at_150_percent() {
    for animations in ["off", "on"] {
        let ctx = Context::default();
        let mut shell = demo_shell(
            &ctx,
            json!({
                "animations": animations,
                "uiScalePercent": 150,
            }),
            false,
            true,
        );
        let size = [1000.0, 680.0];
        let mut t = 1.0;
        for _ in 0..8 {
            t += 0.3;
            step_shell(&mut shell, &ctx, size, t, Vec::new());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            app_ref(&shell).onboarding.view.as_ref().map(|v| v.step),
            Some(Step::Welcome),
            "onboarding Welcome visible ({animations})"
        );
        // Exact 100% coordinates of the Welcome "Next" button in onboarding-glass-dark-100.
        click_at(&mut shell, &ctx, size, &mut t, pos2(677.0, 472.0));
        t += 0.05;
        step_shell(&mut shell, &ctx, size, t, Vec::new());
        assert_eq!(
            app_ref(&shell).onboarding.view.as_ref().map(|v| v.step),
            Some(Step::Features),
            "onboarding Welcome Next button must stay at 100% coordinates (677, 472) at 150% ({animations})"
        );
    }
}

#[test]
fn zoom_shortcut_clears_stuck_hover() {
    let ctx = Context::default();
    let mut shell = demo_shell(
        &ctx,
        json!({
            "animations": "off",
            "uiScalePercent": 100,
        }),
        false,
        false,
    );
    let size = [860.0, 440.0];
    let mut t = 1.0;
    for _ in 0..4 {
        t += 0.05;
        step_shell(&mut shell, &ctx, size, t, Vec::new());
    }
    // Hover over a clickable title-bar tab.
    for _ in 0..2 {
        t += 0.05;
        step_shell(
            &mut shell,
            &ctx,
            size,
            t,
            vec![Event::PointerMoved(pos2(24.0, 18.0))],
        );
    }
    assert!(
        ctx.input(|i| i.pointer.hover_pos()).is_some(),
        "pointer is hovering before zoom shortcut"
    );
    assert!(
        ctx.viewport(|vp| vp
            .hits
            .contains_pointer
            .iter()
            .any(|w| w.sense.senses_click())),
        "a clickable widget is under the pointer before zoom shortcut"
    );
    // Zoom in with Ctrl+= without moving the mouse.
    t += 0.05;
    step_shell(
        &mut shell,
        &ctx,
        size,
        t,
        vec![Event::Key {
            key: Key::Equals,
            physical_key: Some(Key::Equals),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::CTRL,
        }],
    );
    // Advance frames so the new zoom factor applies.
    for _ in 0..2 {
        t += 0.05;
        step_shell(&mut shell, &ctx, size, t, Vec::new());
    }
    assert!(
        crate::zoom::ui(&ctx) > 1.0,
        "Ctrl+= increased ui zoom (got {})",
        crate::zoom::ui(&ctx)
    );
    assert_eq!(
        ctx.input(|i| i.pointer.hover_pos()),
        None,
        "hover must be cleared after a zoom shortcut so the old widget does not stay hovered"
    );
    // A subsequent pointer move restores hover normally.
    t += 0.05;
    step_shell(
        &mut shell,
        &ctx,
        size,
        t,
        vec![Event::PointerMoved(pos2(24.0, 18.0))],
    );
    assert!(ctx.input(|i| i.pointer.hover_pos()).is_some());
    // Ctrl+wheel zoom also clears stuck hover.
    t += 0.05;
    step_shell(
        &mut shell,
        &ctx,
        size,
        t,
        vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 1.0),
            modifiers: Modifiers::CTRL,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        }],
    );
    for _ in 0..2 {
        t += 0.05;
        step_shell(&mut shell, &ctx, size, t, Vec::new());
    }
    assert_eq!(
        ctx.input(|i| i.pointer.hover_pos()),
        None,
        "hover must also be cleared after Ctrl+wheel zoom"
    );
}

#[test]
fn afk_overlay_fades_in_after_idle_frames() {
    let ctx = Context::default();
    let mut shell = demo_shell(
        &ctx,
        json!({
            "animations": "on",
            "uiScalePercent": 150,
            "inactivityBlurSeconds": 1,
        }),
        false,
        false,
    );
    let size = [860.0, 440.0];
    let mut t = 1.0;
    for _ in 0..3 {
        t += 0.05;
        step_shell(&mut shell, &ctx, size, t, Vec::new());
    }
    assert!(
        shell.take_overlay().is_none(),
        "no overlay pass runs while AFK is hidden"
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    t += 0.05;
    step_shell(&mut shell, &ctx, size, t, Vec::new());
    assert!(app_ref(&shell)
        .booted()
        .unwrap()
        .secure
        .afk_overlay_visible());
    let (overlay_ctx, mut overlay_out) = shell
        .take_overlay()
        .expect("AFK overlay pass runs on first visible frame");
    overlay_out.textures_delta.clear();
    let afk_opacity =
        crate::ui::anim::peek::<f32>(&overlay_ctx, egui::Id::unique("afk").with("overlay"))
            .expect("AFK overlay transition state present");
    assert!(
        afk_opacity < 0.5,
        "AFK overlay must fade in from 0 instead of snapping to 1 (got {afk_opacity})"
    );
}

#[test]
fn overlays_keep_hidpi_base_scale_at_150_ui_scale() {
    // Scene-style base zoom (1.5x over a 1.0 native monitor) + 150% UI scale.
    let ctx = Context::default();
    let physical = [1290.0, 660.0];
    // Establish initial screen_rect & base zoom just like SceneRun's Sizing phase.
    crate::zoom::set_base(&ctx, 1.5);
    ctx.set_zoom_factor(1.5);
    let mut init_raw = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(860.0, 440.0))),
        time: Some(0.5),
        ..Default::default()
    };
    init_raw
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap()
        .native_pixels_per_point = Some(1.0);
    let mut init_out = ctx.run_ui(init_raw, |_| {});
    init_out.textures_delta.clear();

    let mut shell = demo_shell(
        &ctx,
        json!({
            "animations": "off",
            "uiScalePercent": 150,
            "pinEnabled": true,
            "pinHash": PIN_HASH_1234,
        }),
        false,
        false,
    );
    let mut t = 1.0;
    for _ in 0..4 {
        t += 0.05;
        step_shell(&mut shell, &ctx, physical, t, Vec::new());
    }
    // Stage runs at base (1.5) * ui (1.5) = 2.25 ppp; overlay must stay at base (1.5).
    assert!(
        (ctx.pixels_per_point() - 2.25).abs() < 1e-4,
        "stage ppp should be 2.25 (got {})",
        ctx.pixels_per_point()
    );
    let (_, mut overlay_out) = shell
        .take_overlay()
        .expect("PIN overlay must render on the unscaled overlay pass");
    overlay_out.textures_delta.clear();
    assert!(
        (overlay_out.pixels_per_point - 1.5).abs() < 1e-4,
        "overlay ppp must stay at HiDPI base 1.5, not 2.25 (got {})",
        overlay_out.pixels_per_point
    );
}
