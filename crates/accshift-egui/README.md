# accshift-egui

The accshift desktop app, drawn with [egui](https://github.com/emilk/egui) 0.36. It builds `accshift-egui`, which the installers ship as `accshift-gui` (`bundle/bundle.mjs`), so an install of a v1.0.x Tauri build updates in place: same identifier, folders, data, deep links and updater.

## Renderer

Feature `software` (default): egui meshes rasterized on the CPU (`vendor/egui_software_backend`) and blitted with softbuffer, GDI on Windows. No GPU driver is loaded, and only the damaged tiles are redrawn. `vendor/epaint` adds one hook to epaint: a font can supply its own glyph coverage, so text is rasterized by DirectWrite on Windows. Feature `glow` is the older eframe on OpenGL path.

## Run

```sh
cargo run -p accshift-egui                                      # the app, on the real data
cargo run -p accshift-egui -- --scene parity/scenes/<id>.json --out <dir>   # scripted shots, hidden window, mock data
node bundle/bundle.mjs --build [--no-updater]                   # installers for the host OS
```

`parity/README.md` describes the scene format. The scenes run on the mock data of `parity/data` and write one PNG per shot; `parity/diff.mjs` compares two sets of shots.

## Startup and memory

Interleaved A/B on Windows 11 against the v1.0.5 Tauri build, hidden windows, real backend on a copy of the same data (`.agents/perf/bench-egui.ps1` in the maintainer checkout). Boot runs from process creation to the first presented frame (Tauri: its `Boot completed` line).

|                    | Tauri 1.0.5 + WebView2                 | egui           |
| ------------------ | -------------------------------------- | -------------- |
| Boot               | 414 ms                                 | 136 ms         |
| Working set        | 482 MB                                 | 134 MB         |
| Private bytes      | 406 MB                                 | 121 MB         |
| Processes          | 7                                      | 1              |
| CPU, boot + 5 s    | 2.34 s                                 | 1.20 s         |
| Shipped files      | exe + 11.9 MB DLL, plus WebView2       | one 16.5 MB exe |

## Layout

| Path             | Role                                                                    |
| ---------------- | ----------------------------------------------------------------------- |
| `src/app/`       | App state, actions, dialogs, the backend jobs                            |
| `src/screens/`   | The workspace, settings, personas and onboarding screens                 |
| `src/ui/`        | Widgets, the CSS box painter, text, animations                           |
| `src/window/`    | The native window: frame, title bar, DPI, acrylic                        |
| `src/harness/`   | The scene runner                                                         |
| `src/soft.rs`    | The software runner: winit loop, damaged-tiles present, pacing           |
| `tauri.conf.json`, `icons/`, `bundle/` | Installer config, read by the Tauri bundler and `build.rs` |
| `vendor/`        | egui_software_backend and epaint, patched                                |
