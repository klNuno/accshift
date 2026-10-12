// Reference frames for the egui parity bench: the real Tauri app (WebView2)
// on the mock frontend, driven over CDP, one PNG per `shot` of each scene.
//
//   cargo build -p accshift-gui --features parity-capture
//   node crates/accshift-egui/parity/ref.mjs crates/accshift-egui/parity/scenes/*.json --out <dir>
//
// Options: --out <dir> (required), --exe <path> (default: the debug
// accshift-gui of the cargo target dir), --scratch <dir> (app dirs, WebView2
// profile and binary copy, removed after the run; default <out>/.scratch),
// --window hidden|cloak (default hidden: the window is never shown).
//
// Output: <out>/<scene id>/<shot>.png at viewport x scale, <out>/manifest.json
// (every shot, with its `alias` if the scene gives one, read by
// diff.mjs --batch), <out>/run.json (window samples and the real-dir check).
//
// Scene format: README.md next to this file. What a scene sets up:
// - the mock: the dev server runs `VITE_MOCK=dev` (not frozen), and the
//   scene's scenario, os, theme, locale and seed are written into
//   `accshift.mock.overrides` in localStorage before each load. `theme` and
//   `locale` patch `client.settings` (`themeId`, `language`); a `seed` key is a
//   store id ("client.view-mode") or the suffix of its CLIENT_STORE_ constant
//   ("SETTINGS"), and an object value is merged over the scenario's store.
// - the clock: `Date.now()` and `new Date()` start at the mock's NOW
//   (1_791_000_000 s) and advance with `performance.now()` (PIN_CLOCK in
//   harness.mjs).
// - the stand-in desktop of the glass themes, painted as the `html`
//   background. It is the README recorder's (`WALLPAPER` and `DESKTOP` in
//   .agents/scripts/record-demo.mjs) and crates/accshift-egui/src/backdrop.rs
//   draws the same thing: a desktop of the viewport plus PAD = 36 CSS px on
//   each side, the window at 36,36, so the page sees the desktop shifted by
//   -36,-36. Layers, top first:
//     radial-gradient(48% 62% at 16% 20%, rgba(109,40,217,.42), transparent 72%)
//     radial-gradient(46% 60% at 86% 84%, rgba(29,78,216,.40), transparent 72%)
//     radial-gradient(34% 44% at 72% 10%, rgba(190,24,93,.22), transparent 72%)
//     radial-gradient(38% 48% at 28% 96%, rgba(8,145,178,.22), transparent 72%)
//     linear-gradient(160deg, #10121f, #0a0c16)
// - input: CDP Input.dispatchMouseEvent / dispatchKeyEvent, so :hover, focus
//   and keyboard handlers behave as under a real hand.
// - `html` (optional): a fragment file, relative to the scene file, that
//   replaces the body once the scene has settled. The primitives scenes use it
//   to photograph plain CSS boxes; the egui side draws the same file.

import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import {
  DEV_URL,
  PIN_CLOCK,
  defaultExe,
  deepLinkRegistration,
  diffTrees,
  installedAppRunning,
  launchApp,
  realAppDirs,
  sleep,
  snapshotTree,
  startVite,
  windowWatch,
} from "./harness.mjs";

const PAD = 36;
const WALLPAPER = [
  "radial-gradient(48% 62% at 16% 20%, rgba(109,40,217,.42), transparent 72%)",
  "radial-gradient(46% 60% at 86% 84%, rgba(29,78,216,.40), transparent 72%)",
  "radial-gradient(34% 44% at 72% 10%, rgba(190,24,93,.22), transparent 72%)",
  "radial-gradient(38% 48% at 28% 96%, rgba(8,145,178,.22), transparent 72%)",
  "linear-gradient(160deg, #10121f, #0a0c16)",
].join(",");

const desktop = (vp) => `
(() => {
  const style = document.createElement("style");
  style.id = "__parity_desktop";
  style.textContent = "html{background:${WALLPAPER} !important;" +
    "background-size:${vp.width + 2 * PAD}px ${vp.height + 2 * PAD}px !important;" +
    "background-position:-${PAD}px -${PAD}px !important;background-repeat:no-repeat !important}";
  const add = () => document.head.append(style);
  if (document.head) add();
  else document.addEventListener("DOMContentLoaded", add);
})();
`;

// Writes the scene's mock state, then the page reloads into it. Runs inside
// the page so the scenario's own stores come from the same modules the app
// imports.
const applyMock = (scene) => `
(async () => {
  const scene = ${JSON.stringify(scene)};
  const { SCENARIOS } = await import("/src/lib/mock/scenarios.ts");
  const ids = await import("/src/lib/storage/storeIds.ts");
  if (!SCENARIOS[scene.scenario]) throw new Error("unknown scenario " + scene.scenario);
  const base = SCENARIOS[scene.scenario]().stores;
  const plain = (v) => v && typeof v === "object" && !Array.isArray(v);
  const stores = {};
  const settings = { ...(base["client.settings"] ?? {}) };
  if (scene.theme) settings.themeId = scene.theme;
  if (scene.locale) settings.language = scene.locale;
  stores["client.settings"] = settings;
  for (const [key, value] of Object.entries(scene.seed ?? {})) {
    const id = ids["CLIENT_STORE_" + key] ?? key;
    const prev = stores[id] ?? base[id];
    stores[id] = plain(value) && plain(prev) ? { ...prev, ...value } : value;
  }
  localStorage.clear();
  sessionStorage.clear();
  const overrides = { scenario: scene.scenario, stores };
  if (scene.os) overrides.runtimeOs = scene.os;
  localStorage.setItem("accshift.mock.overrides", JSON.stringify(overrides));
  return Object.keys(stores);
})()
`;

// Settled: a new document (the one before the reload carries __parityStale),
// on the scenario asked for, past its boot entrance (`.app-frame.boot-ready`
// fully opaque, no finite animation still running), fonts in, every image
// decoded and none blurred under the startup avatar refresh.
const SETTLED = (scenario) => `
(() => {
  if (window.__parityStale) return false;
  const state = window.__mock?.state?.();
  if (!state || state.scenario !== ${JSON.stringify(scenario)}) return false;
  if (document.readyState !== "complete" || document.fonts.status !== "loaded") return false;
  const frame = document.querySelector(".app-frame.boot-ready");
  if (!frame || getComputedStyle(frame).opacity !== "1") return false;
  const finite = document.getAnimations().filter(
    (a) => a.playState === "running" && a.effect?.getComputedTiming().endTime !== Infinity,
  );
  if (finite.length) return false;
  if (!document.querySelector(".card, .row, .empty-state, [class*=empty]")) return false;
  const imgs = [...document.querySelectorAll("img")];
  return imgs.every((i) => i.complete && i.naturalWidth > 0 && !i.classList.contains("blurred"))
    && !document.querySelector(".card .spinner, .card [class*=spin]");
})()
`;

const KEYS = {
  Enter: { code: "Enter", windowsVirtualKeyCode: 13, text: "\r" },
  Escape: { code: "Escape", windowsVirtualKeyCode: 27 },
  Backspace: { code: "Backspace", windowsVirtualKeyCode: 8 },
  Tab: { code: "Tab", windowsVirtualKeyCode: 9 },
  Delete: { code: "Delete", windowsVirtualKeyCode: 46 },
  Home: { code: "Home", windowsVirtualKeyCode: 36 },
  End: { code: "End", windowsVirtualKeyCode: 35 },
  ArrowLeft: { code: "ArrowLeft", windowsVirtualKeyCode: 37 },
  ArrowUp: { code: "ArrowUp", windowsVirtualKeyCode: 38 },
  ArrowRight: { code: "ArrowRight", windowsVirtualKeyCode: 39 },
  ArrowDown: { code: "ArrowDown", windowsVirtualKeyCode: 40 },
  Space: { key: " ", code: "Space", windowsVirtualKeyCode: 32, text: " " },
};
const MODIFIERS = { Alt: 1, Ctrl: 2, Meta: 4, Shift: 8 };

function parseArgs(argv) {
  const opts = { scenes: [], out: null, exe: null, scratch: null, window: "hidden" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--out") opts.out = argv[++i];
    else if (a === "--exe") opts.exe = argv[++i];
    else if (a === "--scratch") opts.scratch = argv[++i];
    else if (a === "--window") opts.window = argv[++i];
    else opts.scenes.push(a);
  }
  if (!opts.out || opts.scenes.length === 0) {
    console.error(
      "usage: ref.mjs <scene.json>... --out <dir> [--exe <path>] [--scratch <dir>] [--window hidden|cloak]",
    );
    process.exit(2);
  }
  return opts;
}

function loadScene(file) {
  const scene = JSON.parse(readFileSync(file, "utf8"));
  scene.id ??= basename(file, ".json");
  scene.scenario ??= "demo";
  scene.viewport ??= { width: 860, height: 440, scale: 1.5 };
  scene.steps ??= [];
  // `html`: a fragment file next to the scene, injected in place of the body
  // once the app has settled (calibration of the egui painter).
  if (scene.html) scene.htmlSource = readFileSync(resolve(dirname(file), scene.html), "utf8");
  return scene;
}

function pngSize(buf) {
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) };
}

async function runScene(page, scene, outDir, manifest) {
  const { send, js } = page;
  const vp = scene.viewport;
  await send("Emulation.setDeviceMetricsOverride", {
    width: vp.width,
    height: vp.height,
    deviceScaleFactor: vp.scale,
    mobile: false,
  });
  const desk = await send("Page.addScriptToEvaluateOnNewDocument", { source: desktop(vp) });

  // The mock import can hit a module still initialising (a reload in flight):
  // reload the dev page and try again before giving up.
  for (let attempt = 1; ; attempt++) {
    try {
      await js(applyMock(scene));
      break;
    } catch (err) {
      if (attempt >= 3) throw err;
      console.error(`${scene.id}: mock setup failed (${err.message}), reloading`);
      await send("Page.navigate", { url: DEV_URL });
      await sleep(1500);
    }
  }
  await js("window.__parityStale = true");
  await send("Page.navigate", { url: DEV_URL });
  let settled = false;
  const started = Date.now();
  while (!settled && Date.now() - started < 30_000) {
    await sleep(100);
    settled = await js(SETTLED(scene.scenario)).catch(() => false);
  }
  if (!settled) console.error(`${scene.id}: page never settled, shooting anyway`);
  else console.log(`${scene.id}: settled after ${Date.now() - started} ms`);
  if (scene.htmlSource) {
    // The app stays mounted on a detached #app; the page shows the fragment.
    await js(
      `(() => { document.activeElement?.blur?.(); document.body.innerHTML = ${JSON.stringify(scene.htmlSource)}; return true; })()`,
    );
    await js(
      "new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))",
    );
  }

  const mouse = { x: 0, y: 0 };
  const mouseEvent = (type, extra = {}) =>
    send("Input.dispatchMouseEvent", { type, x: mouse.x, y: mouse.y, ...extra });
  const moveTo = async ([x, y]) => {
    mouse.x = x;
    mouse.y = y;
    await mouseEvent("mouseMoved");
  };
  const press = async (button, clickCount) => {
    await mouseEvent("mousePressed", { button, clickCount });
    await sleep(40);
    await mouseEvent("mouseReleased", { button, clickCount });
  };
  const stroke = async (spec) => {
    const parts = spec.split("+");
    const name = parts.pop();
    const modifiers = parts.reduce((m, p) => m | (MODIFIERS[p] ?? 0), 0);
    const known = KEYS[name];
    const upper = name.toUpperCase();
    const base = known
      ? { key: name, ...known }
      : name.length === 1
        ? {
            key: name,
            code: /[a-z]/i.test(name) ? `Key${upper}` : /\d/.test(name) ? `Digit${name}` : "",
            windowsVirtualKeyCode: upper.charCodeAt(0),
            text: name,
          }
        : { key: name, code: name };
    const { text, ...rest } = base;
    const typed = text && !(modifiers & (MODIFIERS.Ctrl | MODIFIERS.Alt | MODIFIERS.Meta));
    await send("Input.dispatchKeyEvent", {
      type: typed ? "keyDown" : "rawKeyDown",
      modifiers,
      ...rest,
      ...(typed ? { text, unmodifiedText: text } : {}),
    });
    await send("Input.dispatchKeyEvent", { type: "keyUp", modifiers, ...rest });
  };

  for (const step of scene.steps) {
    if ("wait" in step) await sleep(step.wait);
    else if ("move" in step) await moveTo(step.move);
    else if ("click" in step) {
      await moveTo(step.click);
      await press("left", 1);
    } else if ("dblclick" in step) {
      await moveTo(step.dblclick);
      await press("left", 1);
      await press("left", 2);
    } else if ("rclick" in step) {
      await moveTo(step.rclick);
      await press("right", 1);
    } else if ("key" in step) await stroke(step.key);
    else if ("text" in step) {
      for (const ch of step.text) {
        await stroke(ch === " " ? "Space" : ch);
        await sleep(30);
      }
    } else if ("scroll" in step) {
      const [x, y, dy] = step.scroll;
      mouse.x = x;
      mouse.y = y;
      await mouseEvent("mouseWheel", { deltaX: 0, deltaY: dy });
    } else if ("shot" in step) {
      const { data } = await send("Page.captureScreenshot", { format: "png", fromSurface: true });
      const buf = Buffer.from(data, "base64");
      const dir = join(outDir, scene.id);
      mkdirSync(dir, { recursive: true });
      const file = join(dir, `${step.shot}.png`);
      writeFileSync(file, buf);
      const size = pngSize(buf);
      const want = {
        width: Math.round(vp.width * vp.scale),
        height: Math.round(vp.height * vp.scale),
      };
      const ok = size.width === want.width && size.height === want.height;
      console.log(
        `${scene.id}/${step.shot}.png ${size.width}x${size.height}${ok ? "" : ` (expected ${want.width}x${want.height})`}`,
      );
      manifest.push({
        scene: scene.id,
        shot: step.shot,
        file: `${scene.id}/${step.shot}.png`,
        ...(step.alias ? { alias: step.alias } : {}),
        width: size.width,
        height: size.height,
        sizeOk: ok,
      });
    } else console.error(`${scene.id}: unknown step ${JSON.stringify(step)}`);
  }
  if (scene.probe) await probeText(page, scene, outDir);
  await send("Page.removeScriptToEvaluateOnNewDocument", { identifier: desk.identifier });
}

// `probe` (optional, with `html`): after the shots, every `body p` of the
// fragment is measured and asked which installed face draws it. Writes
// <out>/<id>/probe.json: per line the computed font values, the line box,
// the text's own box (a Range over the contents: advances plus letter
// spacing), the baseline (bottom of a zero-size inline-block appended for
// the measure, then removed) in CSS px, and CSS.getPlatformFontsForNode.
const PROBE = `
(() => [...document.querySelectorAll("body p")].map((p) => {
  const box = p.getBoundingClientRect();
  const range = document.createRange();
  range.selectNodeContents(p);
  const ink = range.getBoundingClientRect();
  const mark = document.createElement("span");
  mark.style.cssText = "display:inline-block;width:0;height:0;vertical-align:baseline";
  p.appendChild(mark);
  const baseline = mark.getBoundingClientRect().bottom;
  mark.remove();
  const cs = getComputedStyle(p);
  return {
    text: p.textContent,
    family: cs.fontFamily,
    size: cs.fontSize,
    weight: cs.fontWeight,
    lineHeight: cs.lineHeight,
    letterSpacing: cs.letterSpacing,
    transform: cs.textTransform,
    box: [box.left, box.top, box.width, box.height],
    text_box: [ink.left, ink.top, ink.width, ink.height],
    baseline,
  };
}))()
`;

async function probeText(page, scene, outDir) {
  const { send, js } = page;
  const lines = await js(PROBE);
  await send("DOM.enable");
  await send("CSS.enable");
  const { root } = await send("DOM.getDocument", { depth: -1 });
  const { nodeIds } = await send("DOM.querySelectorAll", {
    nodeId: root.nodeId,
    selector: "body p",
  });
  for (const [i, nodeId] of nodeIds.entries()) {
    const { fonts } = await send("CSS.getPlatformFontsForNode", { nodeId });
    if (lines[i]) lines[i].platformFonts = fonts;
  }
  await send("CSS.disable");
  await send("DOM.disable");
  const dir = join(outDir, scene.id);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "probe.json"), JSON.stringify(lines, null, 2) + "\n");
  console.log(`${scene.id}/probe.json ${lines.length} lines`);
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const out = resolve(opts.out);
  const scratch = resolve(opts.scratch ?? join(out, ".scratch"));
  mkdirSync(out, { recursive: true });
  mkdirSync(scratch, { recursive: true });
  const scenes = opts.scenes.map(loadScene);
  const exe = resolve(opts.exe ?? defaultExe());

  const dirs = realAppDirs();
  const before = Object.fromEntries(dirs.map((d) => [d, snapshotTree(d)]));
  const regBefore = deepLinkRegistration();
  const watch = windowWatch();

  const vite = await startVite({ logFile: join(out, "vite.log") });
  let app = null;
  const manifest = [];
  try {
    app = await launchApp({
      exe,
      scratch: join(scratch, "run"),
      logFile: join(out, "app.log"),
      onStderr: watch.onLine,
      windowMode: opts.window,
    });
    const { send } = app.page;
    await send("Page.enable");
    await send("Runtime.enable");
    await send("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-reduced-motion", value: "no-preference" }],
    });
    await send("Page.addScriptToEvaluateOnNewDocument", { source: PIN_CLOCK });
    for (const scene of scenes) await runScene(app.page, scene, out, manifest);
  } finally {
    await app?.close();
    await vite.close();
  }

  // The last samples land while the app shuts down. The scratch run (binary
  // copy, app dirs, WebView2 profile) goes with it; app.log stays in <out>.
  await sleep(300);
  rmSync(join(scratch, "run"), { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  const after = Object.fromEntries(dirs.map((d) => [d, snapshotTree(d)]));
  const touched = dirs.flatMap((d) => diffTrees(before[d], after[d]).map((c) => `${d}: ${c}`));
  const regAfter = deepLinkRegistration();
  const win = watch.summary();
  const run = {
    exe,
    window: {
      mode: opts.window,
      ...win,
      samplesLog: watch.samples.slice(0, 3).concat(watch.samples.slice(-2)),
    },
    realDirs: { dirs, filesBefore: dirs.map((d) => Object.keys(before[d]).length), touched },
    installedAppRunning: installedAppRunning(),
    deepLink: { unchanged: regBefore === regAfter, value: regAfter },
    shots: manifest.length,
  };
  writeFileSync(join(out, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  writeFileSync(join(out, "run.json"), JSON.stringify(run, null, 2) + "\n");
  console.log(
    `window: ${win.samples} samples, visible=${win.everVisible} uncloaked=${win.everUncloaked} ` +
      `foreground=${win.everForeground} rect=${win.firstRect?.join(",") ?? "-"} -> hidden=${win.hidden}`,
  );
  console.log(
    `real app dirs: ${touched.length === 0 ? "untouched" : `${touched.length} change(s)`} ` +
      `(${run.realDirs.filesBefore.join(" + ")} files checked), accshift:// ${run.deepLink.unchanged ? "unchanged" : "CHANGED"}`,
  );
  for (const t of touched) console.log(`  ${t}`);
  console.log(
    `${manifest.length} shots, ${manifest.filter((m) => !m.sizeOk).length} with a wrong size -> ${out}`,
  );
  if (!win.hidden || touched.length || !run.deepLink.unchanged || manifest.some((m) => !m.sizeOk)) {
    process.exitCode = 1;
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
