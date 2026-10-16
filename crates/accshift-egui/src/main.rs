#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use accshift_app::real::RealBackend;
use accshift_core::config;
use accshift_egui::{bench, harness, window, AppModel, Screen, Shell};
use raw_window_handle::HasWindowHandle;
use std::sync::Arc;

fn main() {
    let bench_build = cfg!(feature = "startup-bench");
    // `--scene <scene.json>... --out <dir>`: replay parity scenes in a hidden
    // window, see src/harness/mod.rs. Nothing is parsed before main otherwise.
    let (scene_files, out_dir) = {
        let mut files = Vec::new();
        let mut out = None;
        let mut args = std::env::args().skip(1);
        let mut in_scenes = false;
        while let Some(arg) = args.next() {
            if arg == "--scene" {
                in_scenes = true;
            } else if arg == "--out" {
                out = args.next().map(std::path::PathBuf::from);
                in_scenes = false;
            } else if in_scenes {
                files.push(std::path::PathBuf::from(arg));
            }
        }
        (files, out)
    };
    let scene_mode = !scene_files.is_empty();
    let scenes = if scene_mode {
        let Some(out) = out_dir else {
            eprintln!("usage: accshift-egui --scene <scene.json>... --out <dir>");
            std::process::exit(2);
        };
        let mut loaded = Vec::new();
        for file in &scene_files {
            match harness::load(file) {
                Ok(scene) => loaded.push(scene),
                Err(err) => {
                    eprintln!("scene: {err}");
                    std::process::exit(2);
                }
            }
        }
        Some((loaded, out))
    } else {
        None
    };
    let hidden = bench_build || scene_mode;
    // Experiment knob for the boot breakdown: an opaque window without the
    // acrylic, to price the transparent framebuffer.
    let opaque = std::env::var_os("ACCSHIFT_EGUI_OPAQUE").is_some();
    bench::mark_with(
        "main",
        &format!(
            "\"build\":\"{}\",\"renderer\":\"{}\"",
            if bench_build { bench::MARKER } else { "dev" },
            if cfg!(feature = "glow") {
                "glow"
            } else {
                "software"
            },
        ),
    );
    // Debug builds: the window shell on a scratch folder, unseen
    // (window/probe.rs).
    #[cfg(all(debug_assertions, windows))]
    let probe = window::probe::dir();
    #[cfg(not(all(debug_assertions, windows)))]
    let probe: Option<std::path::PathBuf> = None;
    let probing = probe.is_some();
    if hidden || probing {
        // Safety net: a hidden process never outlives its run by much.
        let budget = 60 + 30 * scene_files.len() as u64;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(budget));
            eprintln!("scene: still running after {budget} s, giving up");
            std::process::exit(3);
        });
    }

    // AccessKit initializes UI Automation on the UI thread before the window
    // may show (about 4 ms here). Doing it first on another thread overlaps
    // that with the event loop and window creation.
    #[cfg(windows)]
    std::thread::spawn(|| {
        use windows_sys::Win32::UI::Accessibility::{
            AutomationIdentifierType_Property, ControlType_Property_GUID, UiaLookupId,
        };
        // SAFETY: plain lookup of a built-in property id, the same call
        // accesskit_windows makes to force UIA initialization.
        unsafe {
            UiaLookupId(
                AutomationIdentifierType_Property,
                &ControlType_Property_GUID,
            )
        };
        bench::mark("uia_warm");
    });

    if bench_build {
        // The core is called directly, no IPC: read the real Steam login
        // list (read-only) off the UI thread, to time the link.
        std::thread::spawn(|| {
            let path = std::path::Path::new(r"C:\Program Files (x86)\Steam");
            let count = accshift_core::platforms::steam::accounts::get_accounts(path)
                .map(|accounts| accounts.len())
                .unwrap_or(0);
            bench::mark_with("core_accounts", &format!("\"count\":{count}"));
        });
    }

    // A normal run: the backend on the user's app folders, and the window
    // where Tauri's opens (`build_main_window`).
    let real = (!hidden).then(|| {
        #[cfg(all(debug_assertions, windows))]
        if let Some(dir) = probe {
            return Arc::new(RealBackend::with_context(window::probe::context(dir)));
        }
        match RealBackend::new() {
            Ok(backend) => Arc::new(backend),
            Err(err) => {
                eprintln!("accshift: {err}");
                std::process::exit(1);
            }
        }
    });
    let saved = real
        .as_ref()
        .map(|backend| config::load_window(backend.context().as_ref()));

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Accshift")
        .with_app_id("accshift-egui")
        .with_inner_size([860.0, 440.0])
        .with_decorations(false)
        .with_transparent(!opaque)
        // No file drops in this app, and winit's drop target costs an
        // `OleInitialize` on the UI thread before the window exists.
        .with_drag_and_drop(false)
        .with_resizable(true);
    if let Some(saved) = &saved {
        // The frame (window/frame.rs) moves and sizes it on Windows.
        let (width, height) = saved
            .size
            .unwrap_or((config::DEFAULT_WINDOW_WIDTH, config::DEFAULT_WINDOW_HEIGHT));
        viewport = viewport
            .with_inner_size([width as f32, height as f32])
            .with_min_inner_size([
                config::MIN_WINDOW_WIDTH as f32,
                config::MIN_WINDOW_HEIGHT as f32,
            ]);
    }
    if hidden {
        // Never activated, never in the taskbar, off every monitor, and
        // cloaked below: both runners show the window after its first frame
        // whatever the builder says, so this is what keeps it unseen.
        viewport = viewport
            .with_active(false)
            .with_taskbar(false)
            .with_position([-32000.0, -32000.0]);
    } else if probing {
        // On screen where the frame puts it, but cloaked below.
        viewport = viewport.with_active(false).with_taskbar(false);
    }

    // Shared by both runners once the window exists.
    let build = move |ctx: &egui::Context, handle: &dyn HasWindowHandle| {
        #[cfg(windows)]
        {
            if let (Some(saved), Some(hwnd)) = (&saved, hwnd(handle)) {
                window::frame::install(hwnd, saved);
                #[cfg(debug_assertions)]
                if probing {
                    window::probe::spawn(hwnd);
                }
            }
            // A normal run's backdrop follows the theme (window::Host).
            window_effects(handle, hidden || probing, !opaque && real.is_none());
        }
        #[cfg(not(windows))]
        let _ = (handle, &saved);
        bench::mark("app_created");
        let scenes = scenes.map(|(scenes, out)| {
            std::fs::create_dir_all(&out).ok();
            harness::SceneRun::new(scenes, out)
        });
        match real {
            // The real app folders, through accshift-core.
            Some(backend) => {
                #[cfg(windows)]
                let hwnd = hwnd(handle).map_or(0, |h| h as isize);
                #[cfg(not(windows))]
                let hwnd = 0;
                Shell::new(
                    Screen::App(Box::new(AppModel::real(ctx, Arc::clone(&backend)))),
                    scenes,
                )
                .with_window(window::Host::new(backend, hwnd).focus_on_show(!probing))
            }
            None => {
                // Each scene builds its own screen once the window has its
                // size.
                accshift_egui::screens::main_spike::install_fonts(ctx);
                Shell::new(Screen::Blank, scenes)
            }
        }
    };

    #[cfg(feature = "software")]
    let result = accshift_egui::soft::run(
        viewport,
        Box::new(
            move |ctx: &egui::Context, window: &egui_winit::winit::window::Window| {
                build(ctx, window)
            },
        ),
    )
    .map_err(|err| err.to_string());

    #[cfg(feature = "glow")]
    let result = {
        let options = eframe::NativeOptions {
            viewport,
            persist_window: false,
            // Experiment knob for the boot breakdown: does the first swap
            // wait on the compositor?
            glow_options: eframe::egui_glow::GlowConfiguration {
                vsync: std::env::var_os("ACCSHIFT_EGUI_NO_VSYNC").is_none(),
                ..Default::default()
            },
            ..Default::default()
        };
        eframe::run_native(
            "accshift-egui",
            options,
            Box::new(move |cc| Ok(Box::new(build(&cc.egui_ctx, cc)))),
        )
        .map_err(|err| err.to_string())
    };

    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
    if scene_mode && harness::failures() > 0 {
        eprintln!("scene: {} failure(s)", harness::failures());
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn hwnd(window: &dyn HasWindowHandle) -> Option<windows_sys::Win32::Foundation::HWND> {
    match window.window_handle().ok()?.as_raw() {
        raw_window_handle::RawWindowHandle::Win32(h) => Some(h.hwnd.get() as _),
        _ => None,
    }
}

#[cfg(windows)]
fn window_effects(window: &dyn HasWindowHandle, cloak: bool, acrylic: bool) {
    if let Some(hwnd) = hwnd(window).filter(|_| cloak) {
        let on: i32 = 1;
        // SAFETY: valid HWND owned by the runner, 4-byte BOOL attribute.
        unsafe {
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                windows_sys::Win32::Graphics::Dwm::DWMWA_CLOAK as u32,
                &on as *const i32 as *const _,
                4,
            );
        }
    }
    // Glass Dark's material in Tauri: acrylic with a heavy black tint
    // (`ACRYLIC_TINTS` in src/lib/theme/backdrop.ts).
    if acrylic {
        let _ = window_vibrancy::apply_acrylic(window, Some((0, 0, 0, 150)));
    }
}
