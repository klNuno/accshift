# accshift-egui (spike)

The accshift main screen redrawn in [egui](https://github.com/emilk/egui) 0.36, to weigh a move away from Tauri and WebView2 with numbers and real renders instead of guesses. It is not the app: there are no settings, no personas page, no real switch, and no CI coverage. The root workspace excludes this crate on purpose.

## What it reproduces

- The custom title bar: drag, double-click to maximize, the four left actions, the platform tabs, the caption buttons.
- The Steam grid of the demo dataset (`src/lib/mock/scenarios.ts`): 17 accounts, the "Smurfs" folder, custom card colours, the active rings, the hover lift and avatar zoom, the two-click arm then switch with the blurred avatar, the play icon and the hint, and the 900 ms switch delay.
- Search, which flattens folders like the web grid; the grid and list toggle; the Riot tab (gradient and initials fallback) and the Roblox tab.
- Glass Dark: OS acrylic behind a transparent window, the same tint, fills and text colours as `builtin/glass-dark.json`, and Segoe UI (the webview's fallback, since Inter is not installed).
- `accshift-core` called directly, without IPC: a background thread reads the real Steam login list (read-only) and times it.

## Two renderers

- `software` (default): winit through `egui-winit`, egui's meshes rasterized on the CPU by a vendored copy of [egui_software_backend](https://github.com/DGriffin91/egui_software_backend) 0.0.3 (MIT or Apache-2.0, ported to egui 0.36 in `vendor/`), and blitted with softbuffer, a GDI blit on Windows. No GPU driver is loaded. The runner is `src/soft.rs`.
- `glow`: eframe on OpenGL, the first version of the spike. Build it with `--no-default-features --features glow`.

What the software path does to stay cheap:

- Only the 64 px tiles that changed are rasterized, copied and presented (`present_with_damage`); a full copy happens on the first frame, a resize or an OS repaint.
- Frames are paced to the monitor's refresh rate instead of vsync, and rayon spreads the raster over the tiles.
- Startup: no bundled fonts on Windows (Segoe UI comes from the system), the exe is built for size except the per-frame hot path, the UI Automation, shell and COM DLLs are delay-loaded (`build.rs`), UI Automation warms up on another thread, and window drag and drop stays off so winit skips `OleInitialize`.

## Run it

```powershell
cargo run --release                      # normal window, software renderer
cargo run --release -- --capture <dir>   # scripted shots, hidden window
cargo build --release --features startup-bench
cargo build --release --no-default-features --features glow
```

- `--capture <dir>` runs a fixed script (hover, arm, search, switch, tabs, folder, list) and writes one PAM file per shot, read back from the frame the renderer presented at 860x440 points and 1.5 pixels per point, over the recorder's stand-in desktop (`src/backdrop.rs`). `ACCSHIFT_EGUI_CAPTURE_SCALE` changes the scale. `ffmpeg -i shot.pam shot.png` converts them.
- `startup-bench` never shows the window: it is created inactive, outside the taskbar, at -32000,-32000, and DWM-cloaked before the runner makes it visible. Boot marks go to `%TEMP%\accshift-egui-bench\<pid>.jsonl` (`main`, `window`, `app_created`, `ui_done`, `first_frame`, `ready` once every avatar is on screen, `core_accounts`, and one `frame` line per paint with its timings in the software build). The binary carries the string `accshift-egui-startup-bench-build`, and a harness refuses to launch any build without it.
- `--capture` uses the same hidden-window recipe in any build.
- `ACCSHIFT_EGUI_OPAQUE` (no transparency, no acrylic) and, for glow, `ACCSHIFT_EGUI_NO_VSYNC` exist only for the boot breakdown.

## Results (Windows 11, NVIDIA desktop GPU)

Interleaved A/B against the Tauri `startup-bench` build of the same commit, 15 launches each, first launch dropped, fresh copies of both binaries. Times run from process creation. Tauri's boot is its `Boot completed` log line, written just before it shows the window; egui's is its first frame, already presented. The glow column comes from the same harness one day earlier.

|                             | Tauri 2 + WebView2                                  | egui, software | egui, glow     |
| --------------------------- | --------------------------------------------------- | -------------- | -------------- |
| Boot, median                | 308 ms                                              | 57 ms (min 46) | 324 ms         |
| Every avatar on screen      |                                                     | 66 ms          |                |
| Working set, 3 s after boot | 498 MB                                              | 33 MB          | 103 MB         |
| Private memory              | 431 MB                                              | 21 MB          | 141 MB         |
| Processes                   | 7                                                   | 1              | 1              |
| Shipped files               | exe 238 KB + DLL 11.9 MB, plus the WebView2 runtime | one 4.4 MB exe | one 6.5 MB exe |

- The software build boots about 5 times faster than Tauri, with 15 times less working set and 20 times less private memory. About 31 ms of its boot is spent before `main`, mostly the loader mapping the exe and its DLLs; a bare Rust exe needs 12 ms there. From `main`: window at 12 ms, UI pass done at 15, first frame presented at 21, every avatar at 36.
- Glow does not boot faster than Tauri: about 180 ms go to the window and the WGL context and about 115 ms to its first present, against 3 ms for the UI pass. Vsync and transparency change nothing measurable there, so the cost sits in the OpenGL driver.
- Software frame cost during the capture script, measured on every paint: at 1290x660 pixels, median 0.40 ms, p95 1.6 ms, 8 % of one core; at 2580x1320, median 0.53 ms, p95 5.2 ms, 18 % of one core. The worst frame (73 ms at the bigger size) is the one where every avatar texture arrives.
- The software captures match the glow ones within rounding (PSNR about 51 dB), and every shot is byte-identical between a full redraw and the damaged-tiles path. Both match the Tauri frames of `demo-switch` closely: metrics, colours, rings, the blurred armed avatar and the hint line up. Text is the visible difference, because egui has no variable-font weights (the 500 weight of card names is drawn with the semibold face) and no subpixel positioning like Chromium.
- The spike covers one screen in about 2,700 lines of Rust plus the vendored rasterizer. The Svelte front-end it would replace is about 15,600 lines of Svelte and 23,300 of TypeScript (settings, personas, dialogs, bulk edit, theming, i18n, the theme editor).

Not verified: the software path in a visible window (acrylic through the GDI blit, drag, maximize, live resize), other GPUs and Windows versions, Linux and macOS, accessibility with a screen reader, a wgpu backend.

## Layout

| File              | Role                                                             |
| ----------------- | ---------------------------------------------------------------- |
| `src/app.rs`      | The screen: title bar, header, grid, list, cards, animations     |
| `src/theme.rs`    | Glass Dark tokens, CSS `color-mix`, the avatar fallback gradient |
| `src/data.rs`     | The demo dataset, with the mock SVG avatars embedded             |
| `src/avatars.rs`  | SVG rasterizing and the pre-blurred textures, on a worker thread |
| `src/icons.rs`    | The Lucide and platform icons of the Svelte components           |
| `src/capture.rs`  | The scripted capture run                                         |
| `src/bench.rs`    | Boot marks                                                       |
| `src/backdrop.rs` | The recorder's stand-in desktop, for captures only               |
| `src/soft.rs`     | The software runner: winit loop, damaged-tiles present, pacing   |
| `vendor/`         | egui_software_backend 0.0.3, ported to egui 0.36                 |
| `build.rs`        | Delay-loaded DLLs on Windows                                     |
