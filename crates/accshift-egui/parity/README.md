# Parity bench

Every screen of the egui port is compared, pixel by pixel, with the same screen of the Tauri app. This folder holds the reference side and the comparison:

- `ref.mjs` renders scenes in the real Tauri app (WebView2) on the mock frontend and writes one PNG per shot.
- `diff.mjs` compares reference frames with egui frames and writes a report.
- `dump-data.mjs` exports the mock's command results to `data/<scenario>.json`, so the egui side runs on the same data.
- `harness.mjs` is the shared plumbing: vite dev server, app launch, CDP client, window and data-dir checks.
- `scenes/*.json` are the scenes, `fragments/*.html` the static CSS boxes of the `primitives-*` scenes, `data/*.json` the dumped mock data.

Node 22+ and ffmpeg on `PATH`. No npm dependency.

## Run it

```sh
cargo build -p accshift-gui --features parity-capture
node crates/accshift-egui/parity/ref.mjs crates/accshift-egui/parity/scenes/*.json --out <ref dir>
node crates/accshift-egui/parity/diff.mjs --batch <ref dir> <egui dir> --out <report dir>
node crates/accshift-egui/parity/dump-data.mjs            # every scenario, or name some
```

`ref.mjs` options: `--exe <path>` (default: the debug `accshift-gui` of the cargo target dir), `--scratch <dir>` (default `<out>/.scratch`), `--window hidden|cloak`. It exits non-zero if a shot has the wrong size, if the window was ever seen, or if anything changed in the real app directories or the `accshift://` registration.

## The reference app

A debug `accshift-gui` built with the `parity-capture` feature. That build:

- carries the marker string `accshift-parity-capture-build`; the harness refuses any binary without it, and copies the binary into the scratch folder before launching it;
- never shows its main window (with `--window cloak`: shown without activation as a tool window at -32000,-32000, DWM-cloaked), and prints a `parity-window` line with `IsWindowVisible`, the cloak state, foreground state and window rect every 250 ms; `ref.mjs` turns those into `hidden=true|false`;
- refuses to start without `ACCSHIFT_PARITY_DIR`, and puts every app directory and the WebView2 profile under it;
- registers no single-instance plugin, never rewrites `accshift://`, skips the startup snapshot upgrade and keyring sweep, and runs telemetry with no consent, so nothing is sent.

The frontend comes from `vite dev` on port 1420 (the `devUrl` compiled into the debug app) with `VITE_MOCK=dev`, the non-frozen mock, so each scene picks its own scenario. CDP is opened on a free port through `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, which also turns off Chromium's native occlusion tracking so the unseen window keeps painting.

The scale is real: `ref.mjs` launches the app once per scene scale with `--force-device-scale-factor=<scale>` in the same arguments (`launchApp({ scale })` in `harness.mjs`), which is what a Windows display at that scaling gives, and sets only the viewport size, in CSS px, through `Emulation.setDeviceMetricsOverride` (`deviceScaleFactor: 0`). It refuses a scene whose `devicePixelRatio` differs from its scale. The CDP emulated factor is not used: it lays the page out at the window's own factor and scales the frame, so a 1px border stays 1.5 device px at 150 % and boxes snap to whole CSS px, where a real 150 % display lays out in device pixels (border floored to 1 device px, edges snapped to the device grid). References taken before 2026-10-12 used the emulation and are wrong at 1.25 and 1.5. Two runs at 1.5 give identical frames for the title bar and primitives scenes and 103 to 112 dB on `spike-main`.

## Scene format

`scenes/<id>.json`, the contract between `ref.mjs` and the egui runner (`accshift-egui --scene`):

```json
{
  "id": "main-grid",
  "scenario": "demo",
  "os": "windows",
  "viewport": { "width": 860, "height": 440, "scale": 1.5 },
  "theme": "glass-dark",
  "locale": "en",
  "seed": { "SETTINGS": { "viewMode": "grid" } },
  "steps": [
    { "wait": 600 },
    { "move": [425, 220] },
    { "click": [315, 133] },
    { "dblclick": [260, 17] },
    { "rclick": [315, 133] },
    { "key": "Ctrl+K" },
    { "text": "ra" },
    { "scroll": [430, 300, -120] },
    { "shot": "hover", "alias": "egui-0-hover" }
  ]
}
```

- Coordinates are CSS pixels of the viewport (1 CSS px = 1 egui point). Output: `<out>/<id>/<shot>.png`, viewport times scale.
- `scenario` is a key of `SCENARIOS` in `src/lib/mock/scenarios.ts`; `os` sets the mock's runtime OS.
- `streaming` (optional, `true`) makes `detect_streaming_software` report streaming software running (`__mock.streaming`), so a scene seeded with `streamerMode: "auto"` reaches the streamer overlay.
- `cs2` (optional, an array of `Cs2BridgeAccount` rows) enables the CS2 bridge and answers `cs2_bridge_fetch` and `cs2_bridge_check` with these rows (`__mock.cs2`), so Steam cards show the CS2 badge and section.
- `clipboard` (optional, a string) is the text the clipboard holds: `ref.mjs` makes `navigator.clipboard.readText` answer it, the egui runner answers the app's paste request with it. A scene never reads the machine's clipboard; an empty string reads as a clipboard with no text.
- `theme` and `locale` patch `client.settings` (`themeId`, `language`). A `seed` key is a store id (`client.view-mode`) or the suffix of its `CLIENT_STORE_` constant in `src/lib/storage/storeIds.ts` (`SETTINGS`); an object value is merged over the scenario's own store, anything else replaces it. A seeded string under `src/lib/mock/avatars/` (a `cache.steam.profiles` URL) is the file there: `ref.mjs` turns it into a same-origin URL, egui reads the file.
- `down` presses the primary button at a point and holds it; every `move` until the next `up` (which moves there, then releases) reports the held button, so a scene can drag (the bulk edit paint gesture).
- `key` takes `Ctrl`, `Shift`, `Alt` and `Meta` prefixes joined with `+`, then a character or a key name (`Enter`, `Escape`, `Backspace`, `Tab`, `Delete`, `Home`, `End`, arrow keys, `Space`). `text` types character by character. `scroll` is `[x, y, deltaY]`, with an optional fourth item naming the modifiers held (`"Ctrl"`, joined with `+`). A synthetic Ctrl+wheel from CDP does not reach the app's zoom handler in WebView2, so the `zoom-*` scenes zoom with `Ctrl+=` and `Ctrl+-`.
- A shot waits for nothing: put a `wait` before it, long enough for transitions to end.
- `html` (optional) names a fragment file, relative to the scene file. `ref.mjs` replaces the page body with it once the scene has settled; the egui runner draws the same file with `ui::paint` instead of the app. The `primitives-*` scenes use `fragments/*.html` this way to calibrate the CSS box painter at 1, 1.25 and 1.5 pixels per point. Only absolutely positioned `<div>`s and `<p>`s with inline `style` are drawn on the egui side: a `<div>` is a box (`ui::paint`), a `<p>` one line of text (`ui::text`). The `typography-*` scenes calibrate `ui::text` with every text style of the app.
- `probe` (optional, with `html`): `ref.mjs` also measures every `<p>` of the fragment and writes `<out>/<id>/probe.json` (computed font values, line box, text box, baseline in CSS px, and the faces `CSS.getPlatformFontsForNode` reports). The ignored test `screens::primitives::tests::text_matches_webview2_probe` compares `ui::text` against it: `ACCSHIFT_TEXT_PROBE=<ref out dir> cargo test --release -- --ignored probe`.
- `dom` is a step, `{ "dom": "<name>", "select": "<selector list>" }`: `ref.mjs` writes `<out>/<id>/<name>.dom.json`, every matching element with its border box in CSS px, its focus, `:focus-visible` and `:hover` state, and the computed values listed in `DOM_PROPS`. The egui runner skips it.
- `alias` on a shot is optional: the file name, without extension, of the same shot in an older capture set (the spike's `--capture` PAMs). `diff.mjs --batch` uses it when `<egui dir>/<id>/<shot>` does not exist.

Before each scene, the page is reloaded into its mock state, and runs once every image is decoded and no avatar is blurred.

## What both sides must reproduce

- Clock: `Date.now()` and an argument-less `new Date()` start at the mock's `NOW` (1_791_000_000 s) when the document starts, and advance with `performance.now()`.
- Glass themes: a transparent window has nothing behind it, so the page paints a stand-in desktop as the `html` background, the one of the README recorder (`WALLPAPER` in `.agents/scripts/record-demo.mjs`). The desktop is the viewport plus 36 CSS px on each side, with the window at 36,36; the page sees it shifted by -36,-36. `src/backdrop.rs` draws the same thing. Layers, top first:
  - `radial-gradient(48% 62% at 16% 20%, rgba(109,40,217,.42), transparent 72%)`
  - `radial-gradient(46% 60% at 86% 84%, rgba(29,78,216,.40), transparent 72%)`
  - `radial-gradient(34% 44% at 72% 10%, rgba(190,24,93,.22), transparent 72%)`
  - `radial-gradient(38% 48% at 28% 96%, rgba(8,145,178,.22), transparent 72%)`
  - `linear-gradient(160deg, #10121f, #0a0c16)`
- `prefers-reduced-motion` is emulated as `no-preference`; the mock forces `animations: "on"` anyway.
- UI zoom (`uiScalePercent` seeded in `SETTINGS`): `.app-stage` is scaled from its top left, its width divided by the zoom and its height the window's (it is `flex: 1`). The stage rastered during the boot entrance is drawn bilinearly scaled until the zoom changes; with no entrance, or after a change, it is rastered sharp at its final size. `src/zoom.rs` reproduces both. The `zoom-*` scenes cover 90, 110 and 125 at 1 and 1.5 and the shortcuts; a shortcut leaves Chromium's hover on the element under the pointer before the change, so those scenes park the pointer on the title bar.

## Diff metrics

Both images are decoded to RGBA by ffmpeg (PNG or PAM) and composited over black. Per shot: mean absolute error over RGB (0 to 255), PSNR over RGB in dB (99 when identical), percent of pixels whose largest channel delta exceeds 8 and 24, the worst 16x16 tile with its mean error, a heat map (`<shot>-heat.png`, largest channel delta times 4 on a black, red, yellow, white ramp) and a side-by-side (`<shot>-side.png`, reference on top). Batch mode writes `report.json` and `report.md`, one row per shot.

`--region x,y,w,h` limits every number and both images to one rectangle, in CSS px of the viewport. Batch mode scales it by each scene's `viewport.scale` (read from `scenes/<scene>.json`, a scene without a file is skipped), a single pair by `--scale <s>` (default 1); edges are rounded, so `0,0,860,36` is 54 device rows at 1.5. The worst tile is reported in image pixels of the full frame. The title bar alone: `node parity/diff.mjs --batch <ref> <egui> --out <dir> --region 0,0,860,36`.

`ref.mjs` rewrites `<out>/manifest.json` with the scenes of the current run only: capturing more scenes into an existing reference folder drops the earlier ones from the manifest. Use a fresh folder per run, or delete the manifest so `diff.mjs` pairs every PNG it finds.

## Device-pixel snapping

Where WebView2 puts a box at a real device scale factor is measured, not assumed: `snap/measure.mjs` renders a fragment of 25 cases (backgrounds, borders, radii, shadows, translations, `will-change`, flex centring, SVG roots, `img`, `opacity`) at six fractional CSS offsets, in the real app at each scale, and reads the painted edges from the frame.

```sh
node crates/accshift-egui/parity/snap/measure.mjs --out <dir> --exe <parity-capture accshift-gui> --scales 1,1.25,1.5,1.75
```

It writes `cases.json` (layout rect and painted edges per case, offset and scale), `report.md`, one `frame.png` per scale and `titlebar.json` (the title bar's boxes on the `demo` scenario). `--dsf emulate` measures the CDP emulation instead, which is what the references were before 2026-10-12. Same safety as `ref.mjs`.

The rules below come from `measure.mjs` at 1, 1.25, 1.5 and 1.75. Boxes, borders, shadows, `img`, SVG roots, `opacity` and flex centring match on all 24 samples per case; the translated cases match up to the 2-decimal rounding of `cases.json`, and `shadow-rounded` is read across the corner arc, so its scan is not an edge. `src/ui/snap.rs` implements the rules, and `ui::paint` and the components use them. Layout runs in device px with 1/64 px units, so a position is `css * scale` quantized to 1/64 before any rounding.

| Case                                                                                                | Rule                                                                                                                                                |
| --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| Background, rounded box, `box-shadow` base, `img`, inline `svg` root, `opacity`, flex-centred child | Each edge rounded half up on its own: `round(x)` to `round(x + w)` (`snap::rect`).                                                                  |
| `border-width`                                                                                      | Floored to whole device px, 1 minimum when non-zero: 1 CSS px is 1 device px at 1.25, 1.5 and 1.75, 2 CSS px is 2, 3 and 3 (`snap::border_device`). |
| `box-shadow`                                                                                        | The snapped border box plus `offset * scale`, blurred; a 4 px offset is whole at every scale tested.                                                |
| `transform: translate()`, `will-change: transform`, running animation                               | The untransformed box snapped, then the translation added exactly: a fractional translation paints anti-aliased edges (`snap::translated`).         |
| `will-change: transform` without translation                                                        | Origin rounded, size rounded from the origin.                                                                                                       |
| Contents of a scroll container with real overflow (the title bar `.tabs`, whose tooltips overflow)  | Composited: the whole offset, translation included, rounded (`snap::composited`). Measured on the title bar at 1.25, 1.5 and 1.75.                  |
| Flex centring                                                                                       | Free space halved in 1/64 px units, then the child snapped as a box (`snap::centre`).                                                               |

Ban warnings only reach the first scene of each scale: in the same app run, later scenes show no ban outline and no warning section, though each scene reloads the page on a fresh mock. The cause is still open. Capture a scene that shows bans alone (`ref.mjs <scene> --out <fresh folder>`); the `cardext-*` scenes need it.

## Mock data

`data/<scenario>.json`, format 1: `spec` (label, runtime OS, switch delay, Steam path, API key flag, current account per platform), `commands` (per command, a list of `{ args, result }` or `{ args, error }`), `skipped` (commands with state that are modelled from `spec` instead: the switch and the PIN session). Platform commands are called once per platform id, per-account commands once per account. Avatar URLs are repo-relative paths under `src/lib/mock/avatars/`, cache timestamps are exactly `NOW * 1000`. The full description sits at the top of `dump-data.mjs`.
