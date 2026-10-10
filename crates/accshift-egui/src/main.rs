#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use accshift_egui::{bench, capture::Capture, AccshiftApp, Options, Shell};

fn main() -> eframe::Result {
    let bench_build = cfg!(feature = "startup-bench");
    // `--capture <dir>`: scripted offscreen shots, see src/capture.rs.
    let capture_dir = {
        let mut args = std::env::args().skip(1);
        let mut dir = None;
        while let Some(arg) = args.next() {
            if arg == "--capture" {
                dir = args.next().map(std::path::PathBuf::from);
            }
        }
        dir
    };
    let hidden = bench_build || capture_dir.is_some();
    // Experiment knob for the boot breakdown: an opaque window without the
    // acrylic, to price the transparent framebuffer.
    let opaque = std::env::var_os("ACCSHIFT_EGUI_OPAQUE").is_some();
    bench::mark_with(
        "main",
        &format!(
            "\"build\":\"{}\"",
            if bench_build { bench::MARKER } else { "dev" }
        ),
    );
    if hidden {
        // Safety net: a hidden process never outlives its run by much.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(60));
            std::process::exit(0);
        });
    }

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
        .with_min_inner_size([520.0, 320.0])
        .with_decorations(false)
        .with_transparent(!opaque)
        .with_resizable(true);
    if hidden {
        // Never activated, never in the taskbar, off every monitor, and
        // cloaked below: eframe shows the window after its first frame
        // whatever the builder says, so this is what keeps it unseen.
        viewport = viewport
            .with_active(false)
            .with_taskbar(false)
            .with_position([-32000.0, -32000.0]);
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: false,
        // Experiment knob for the boot breakdown: does the first swap wait
        // on the compositor?
        glow_options: eframe::egui_glow::GlowConfiguration {
            vsync: std::env::var_os("ACCSHIFT_EGUI_NO_VSYNC").is_none(),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "accshift-egui",
        options,
        Box::new(move |cc| {
            #[cfg(windows)]
            window_effects(cc, hidden, !opaque);
            bench::mark("app_created");
            let capture = capture_dir.map(|dir| {
                std::fs::create_dir_all(&dir).ok();
                // The Tauri recorder's scale: 860x440 points at 1.5.
                let native = cc.egui_ctx.native_pixels_per_point().unwrap_or(1.0);
                cc.egui_ctx.set_zoom_factor(1.5 / native);
                cc.egui_ctx
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(860.0, 440.0)));
                Capture::new(dir)
            });
            let options = Options {
                desktop_backdrop: capture.is_some(),
            };
            Ok(Box::new(Shell {
                app: AccshiftApp::new(&cc.egui_ctx, options),
                capture,
            }))
        }),
    )
}

#[cfg(windows)]
fn window_effects(cc: &eframe::CreationContext<'_>, cloak: bool, acrylic: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if cloak {
        if let Ok(handle) = cc.window_handle() {
            if let RawWindowHandle::Win32(h) = handle.as_raw() {
                let on: i32 = 1;
                // SAFETY: valid HWND owned by eframe, 4-byte BOOL attribute.
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
        let _ = window_vibrancy::apply_acrylic(cc, Some((0, 0, 0, 150)));
    }
}
