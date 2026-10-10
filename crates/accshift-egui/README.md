# accshift-egui (spike)

The accshift main screen redrawn in [egui](https://github.com/emilk/egui) 0.36 (eframe, glow backend), to weigh a move away from Tauri and WebView2 with numbers and real renders instead of guesses. It is not the app: there are no settings, no personas page, no real switch, and no CI coverage. The root workspace excludes this crate on purpose.

## What it reproduces

- The custom title bar: drag, double-click to maximize, the four left actions, the platform tabs, the caption buttons.
- The Steam grid of the demo dataset (`src/lib/mock/scenarios.ts`): 17 accounts, the "Smurfs" folder, custom card colours, the active rings, the hover lift and avatar zoom, the two-click arm then switch with the blurred avatar, the play icon and the hint, and the 900 ms switch delay.
- Search, which flattens folders like the web grid; the grid and list toggle; the Riot tab (gradient and initials fallback) and the Roblox tab.
- Glass Dark: OS acrylic behind a transparent window, the same tint, fills and text colours as `builtin/glass-dark.json`, and Segoe UI (the webview's fallback, since Inter is not installed).
- `accshift-core` called directly, without IPC: a background thread reads the real Steam login list (read-only) and times it.

## Run it

```powershell
cargo run --release                      # normal window
cargo run --release -- --capture <dir>   # scripted shots, hidden window
cargo build --release --features startup-bench
```

- `--capture <dir>` runs a fixed script (hover, arm, search, switch, tabs, folder, list) and writes one PAM file per shot, read back from the GL framebuffer of the real renderer at 860x440 points and 1.5 pixels per point, over the recorder's stand-in desktop (`src/backdrop.rs`). `ffmpeg -i shot.pam shot.png` converts them.
- `startup-bench` never shows the window: it is created inactive, outside the taskbar, at -32000,-32000, and DWM-cloaked before eframe makes it visible. Boot marks go to `%TEMP%\accshift-egui-bench\<pid>.jsonl` (`main`, `app_created`, `ui_done`, `first_frame`, `ready` once every avatar is on screen, `core_accounts`). The binary carries the string `accshift-egui-startup-bench-build`, and a harness refuses to launch any build without it.
- `--capture` uses the same hidden-window recipe in any build.
- Two environment knobs exist only for the boot breakdown: `ACCSHIFT_EGUI_NO_VSYNC` and `ACCSHIFT_EGUI_OPAQUE` (no transparency, no acrylic).

## Results (2026-10-10, Windows 11, NVIDIA desktop GPU)

Interleaved A/B against the Tauri `startup-bench` build of the same commit, 15 launches each, first launch dropped, fresh copies of both binaries. Tauri's boot is its `Boot completed` log line, written just before it shows the window; egui's is its first frame, already presented.

|                               | Tauri 2 + WebView2                                  | egui 0.36 + glow |
| ----------------------------- | --------------------------------------------------- | ---------------- |
| Boot, median (quiet machine)  | 285 ms                                              | 324 ms           |
| Boot, median (loaded machine) | 435 ms                                              | 468 ms           |
| Working set, 3 s after boot   | 502 MB                                              | 103 MB           |
| Private memory                | 434 MB                                              | 141 MB           |
| Processes                     | 7                                                   | 1                |
| Shipped files                 | exe 238 KB + DLL 11.9 MB, plus the WebView2 runtime | one 6.5 MB exe   |

- Memory and process count drop by a factor of 4 to 7. Boot does not improve: egui spends about 180 ms creating the window and the WGL context and about 115 ms on its first paint and present, against 3 ms for the UI pass itself. Turning vsync off or making the window opaque without acrylic changes nothing measurable, so the cost sits in the OpenGL driver.
- The renders match the Tauri frames of `demo-switch` closely: metrics, colours, rings, the blurred armed avatar and the hint all line up. Text is the visible difference, because egui has no variable-font weights (the 500 weight of card names is drawn with the semibold face) and no subpixel positioning like Chromium.
- The spike covers one screen in about 2,200 lines of Rust. The Svelte front-end it would replace is about 15,600 lines of Svelte and 23,300 of TypeScript (settings, personas, dialogs, bulk edit, theming, i18n, the theme editor).

Not measured: a wgpu backend (DX12 might create its device faster than the NVIDIA OpenGL driver, but a transparent DX12 window needs DirectComposition), other GPUs, real input in a visible window, accessibility with a screen reader.

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
