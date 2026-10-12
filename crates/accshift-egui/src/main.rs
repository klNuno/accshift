#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use accshift_egui::{bench, harness, AccshiftApp, Options, Screen, Shell};
use raw_window_handle::HasWindowHandle;

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
    if hidden {
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

    // The core is called directly, no IPC: read the real Steam login list
    // (read-only) off the UI thread, to prove the link and time it.
    std::thread::spawn(|| {
        let path = std::path::Path::new(r"C:\Program Files (x86)\Steam");
        let count = accshift_core::platforms::steam::accounts::get_accounts(path)
            .map(|accounts| accounts.len())
            .unwrap_or(0);
        bench::mark_with("core_accounts", &format!("\"count\":{count}"));
    });

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Accshift (egui spike)")
        .with_app_id("accshift-egui")
        .with_inner_size([860.0, 440.0])
        .with_decorations(false)
        .with_transparent(!opaque)
        // No file drops in this app, and winit's drop target costs an
        // `OleInitialize` on the UI thread before the window exists.
        .with_drag_and_drop(false)
        .with_resizable(true);
    if !scene_mode {
        // A scene sets its own viewport, which may be smaller.
        viewport = viewport.with_min_inner_size([520.0, 320.0]);
    }
    if hidden {
        // Never activated, never in the taskbar, off every monitor, and
        // cloaked below: both runners show the window after its first frame
        // whatever the builder says, so this is what keeps it unseen.
        viewport = viewport
            .with_active(false)
            .with_taskbar(false)
            .with_position([-32000.0, -32000.0]);
    }

    // Shared by both runners once the window exists.
    let build = move |ctx: &egui::Context, window: &dyn HasWindowHandle| {
        #[cfg(windows)]
        window_effects(window, hidden, !opaque);
        #[cfg(not(windows))]
        let _ = window;
        bench::mark("app_created");
        let scenes = scenes.map(|(scenes, out)| {
            std::fs::create_dir_all(&out).ok();
            harness::SceneRun::new(scenes, out)
        });
        let screen = if scenes.is_some() {
            // Each scene builds its own screen once the window has its size.
            accshift_egui::screens::main_spike::install_fonts(ctx);
            Screen::Blank
        } else {
            Screen::Main(Box::new(AccshiftApp::new(
                ctx,
                Options {
                    desktop_backdrop: false,
                },
            )))
        };
        Shell { screen, scenes }
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
fn window_effects(window: &dyn HasWindowHandle, cloak: bool, acrylic: bool) {
    use raw_window_handle::RawWindowHandle;
    if cloak {
        if let Ok(handle) = window.window_handle() {
            if let RawWindowHandle::Win32(h) = handle.as_raw() {
                let on: i32 = 1;
                // SAFETY: valid HWND owned by the runner, 4-byte BOOL attribute.
                unsafe {
                    windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                        h.hwnd.get() as _,
                        windows_sys::Win32::Graphics::Dwm::DWMWA_CLOAK as u32,
                        &on as *const i32 as *const _,
                        4,
                    );
                }
            }
        }
    }
    // Glass Dark's material in Tauri: acrylic with a heavy black tint
    // (`ACRYLIC_TINTS` in src/lib/theme/backdrop.ts).
    if acrylic {
        let _ = window_vibrancy::apply_acrylic(window, Some((0, 0, 0, 150)));
    }
}
