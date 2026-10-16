// Device-pixel snapping probe: where WebView2 paints a box, a border, a
// shadow, an SVG root, an image, compared with where layout puts it.
//
//   node crates/accshift-egui/parity/snap/measure.mjs --out <dir> [--exe <path>] [--scales 1,1.25,1.5,1.75] [--dsf emulate|force]
//
// One generated fragment (written to <out>/snap.html): a grid of cases, one
// row per case, one column per fractional CSS offset (0, .25, .3, .5, .7,
// .75 added to whole-pixel cell origins), every case white on a black page.
// At each scale the fragment replaces the page body, then for every
// `[data-case]` element the script records `getBoundingClientRect()` (layout,
// CSS px) and reads the painted edges from a screenshot: along the row and
// the column through the element's centre, the coverage of a device pixel is
// its green level over the case's full level (`data-fg`, 255 by default;
// Skia blends in gamma space, so coverage is linear in the stored value).
//
// Per axis a case is measured one of three ways (`data-h`, `data-v`):
//   solid   a filled span: left = s + sum(1 - c) up to the centre, right =
//           centre + sum(c) after it
//   start   a band in the first half (a left or top border): its coverage
//           centroid and its width sum(c), so its outer edge is
//           centroid - width / 2
//   end     a band in the second half (a bottom border, an offset shadow)
//
// `--dsf force` (default) launches the app once per scale with the real
// device scale factor (launchApp `scale`, as ref.mjs does); `--dsf emulate`
// keeps one app and sets the factor through CDP emulation, which lays the
// page out at the window's own factor and scales the frame (kept to show the
// difference).
//
// It also boots the app on the `demo` scenario and records the rects of the
// title bar's boxes and SVG roots at each scale (`<out>/titlebar.json`).
//
// Output: <out>/<scale>/frame.png, <out>/cases.json (every measure),
// <out>/report.md (one table per case). Same safety as ref.mjs: hidden
// window, scratch app dirs, real app dirs and `accshift://` checked.

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { crc32, deflateSync } from "node:zlib";
import {
  DEV_URL,
  PIN_CLOCK,
  defaultExe,
  deepLinkRegistration,
  diffTrees,
  launchApp,
  realAppDirs,
  sleep,
  snapshotTree,
  startVite,
  windowWatch,
} from "../harness.mjs";

const OFFSETS = [0, 0.25, 0.3, 0.5, 0.7, 0.75];
const CELL_W = 64;
const CELL_H = 40;
const LEFT = 8;
const TOP = 8;

// A white PNG (avatars are <img>), 20x10.
function whitePng(w, h) {
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type, "latin1"), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(td) >>> 0);
    return Buffer.concat([len, td, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  const raw = Buffer.alloc((w * 3 + 1) * h, 255);
  for (let y = 0; y < h; y++) raw[y * (w * 3 + 1)] = 0;
  const png = Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
  return `data:image/png;base64,${png.toString("base64")}`;
}

const SVG12 =
  '<svg width="12" height="12" viewBox="0 0 12 12"><rect width="12" height="12" fill="#fff"/></svg>';

// Each case: html(x, y, f) for a cell whose whole-pixel origin is x, y and
// whose fractional offset is f. The measured element carries data-case.
const CASES = [
  {
    id: "bg",
    note: "square box background",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;background:#fff"></div>`,
  },
  {
    id: "bg-fracsize",
    note: "square box, fractional size 30.4x16.4",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30.4px;height:16.4px;background:#fff"></div>`,
  },
  {
    id: "border-all",
    note: "square box, 1px border on all sides, no fill",
    h: "start",
    v: "start",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;border:1px solid #fff"></div>`,
  },
  {
    id: "border-one",
    note: "square box, 1px border-left and border-bottom only",
    h: "start",
    v: "end",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;border-left:1px solid #fff;border-bottom:1px solid #fff"></div>`,
  },
  {
    id: "border-2",
    note: "square box, 2px border, no fill",
    h: "start",
    v: "start",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;border:2px solid #fff"></div>`,
  },
  {
    id: "rounded",
    note: "rounded box (radius 6) background",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;background:#fff;border-radius:6px"></div>`,
  },
  {
    id: "rounded-border",
    note: "rounded box (radius 6), 1px border, no fill",
    h: "start",
    v: "start",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;border:1px solid #fff;border-radius:6px"></div>`,
  },
  {
    id: "shadow",
    note: "square box, box-shadow 4px 4px 0 white, no fill (the strip right of and under the box)",
    h: "end",
    v: "end",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;box-shadow:4px 4px 0 #fff"></div>`,
  },
  {
    id: "shadow-rounded",
    note: "rounded box (radius 6), box-shadow 4px 4px 0 white, no fill",
    h: "end",
    v: "end",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;border-radius:6px;box-shadow:4px 4px 0 #fff"></div>`,
  },
  {
    id: "translate",
    note: "square box at a whole left/top, transform: translate(f, f)",
    html: (x, y, f) =>
      `<div data-case style="left:${x}px;top:${y}px;width:30px;height:16px;background:#fff;transform:translate(${f}px,${f}px)"></div>`,
  },
  {
    id: "translate-rounded",
    note: "rounded box (radius 4), transform: translate(f, f)",
    html: (x, y, f) =>
      `<div data-case style="left:${x}px;top:${y}px;width:30px;height:16px;background:#fff;border-radius:4px;transform:translate(${f}px,${f}px)"></div>`,
  },
  {
    id: "will-change",
    note: "square box, will-change: transform, at a fractional left/top",
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;background:#fff;will-change:transform"></div>`,
  },
  {
    id: "will-change-translate",
    note: "square box, will-change: transform, transform: translate(f, f)",
    html: (x, y, f) =>
      `<div data-case style="left:${x}px;top:${y}px;width:30px;height:16px;background:#fff;will-change:transform;transform:translate(${f}px,${f}px)"></div>`,
  },
  {
    id: "animated",
    note: "square box, running transform animation holding translate(f, f)",
    html: (x, y, f) =>
      `<div data-case style="left:${x}px;top:${y}px;width:30px;height:16px;background:#fff;--f:${f}px;animation:snap-hold 1000s linear infinite"></div>`,
  },
  {
    id: "flex-center",
    note: "flex row 31x17 + f at x + f, centring a 20x10 child (remainder 5.5, 3.5)",
    html: (x, y, f) =>
      `<div class="flex" style="left:${x + f}px;top:${y + f}px;width:31px;height:17px"><div data-case class="static" style="width:20px;height:10px;background:#fff"></div></div>`,
  },
  {
    id: "flex-translate",
    note: "like .tabs: left/top 50% of a 36px bar, translate(-50%, -50%), a 32x28 child",
    html: (x, y, f) =>
      `<div class="bar" style="left:${x}px;top:${y - 4}px;width:${46 + f * 2}px;height:36px"><div class="tabs"><div data-case class="static" style="width:32px;height:28px;background:#fff"></div></div></div>`,
  },
  {
    id: "overflow-translate",
    note: "overflow-x: auto box translated by (f, f), its white 20x10 child measured",
    html: (x, y, f) =>
      `<div style="left:${x}px;top:${y}px;width:30px;height:16px;overflow-x:auto;transform:translate(${f}px,${f}px)"><div data-case class="static" style="width:20px;height:10px;background:#fff"></div></div>`,
  },
  {
    id: "hidden-translate",
    note: "overflow: hidden box translated by (f, f), its white 20x10 child measured",
    html: (x, y, f) =>
      `<div style="left:${x}px;top:${y}px;width:30px;height:16px;overflow:hidden;transform:translate(${f}px,${f}px)"><div data-case class="static" style="width:20px;height:10px;background:#fff"></div></div>`,
  },
  {
    id: "tabs-overflow",
    note: "the title bar's .tabs (overflow-x: auto, translate(-50%, -50%)) in a 36px bar, a 33x28 child",
    html: (x, y, f) =>
      `<div class="bar" style="left:${x}px;top:${y - 4}px;width:${46 + f * 2}px;height:36px"><div class="tabs" style="overflow-x:auto"><div data-case class="static" style="width:33px;height:28px;background:#fff"></div></div></div>`,
  },
  {
    id: "svg-flex",
    note: "inline SVG 12x12 (display:block) centred in a 31x17 flex box at x + f",
    html: (x, y, f) =>
      `<div class="flex" style="left:${x + f}px;top:${y + f}px;width:31px;height:17px">${SVG12.replace("<svg", "<svg data-case")}</div>`,
  },
  {
    id: "svg-abs",
    note: "inline SVG 12x12, absolutely positioned at x + f",
    html: (x, y, f) =>
      SVG12.replace(
        "<svg",
        `<svg data-case style="position:absolute;left:${x + f}px;top:${y + f}px"`,
      ),
  },
  {
    id: "svg-translate",
    note: "inline SVG 12x12 at a whole left/top, transform: translate(f, f)",
    html: (x, y, f) =>
      SVG12.replace(
        "<svg",
        `<svg data-case style="position:absolute;left:${x}px;top:${y}px;transform:translate(${f}px,${f}px)"`,
      ),
  },
  {
    id: "img",
    note: "img 20x10 (white PNG) at x + f",
    html: (x, y, f) =>
      `<img data-case src="${whitePng(20, 10)}" style="position:absolute;left:${x + f}px;top:${y + f}px;width:20px;height:10px">`,
  },
  {
    id: "img-rounded",
    note: "img 20x10, border-radius 4, at x + f",
    html: (x, y, f) =>
      `<img data-case src="${whitePng(20, 10)}" style="position:absolute;left:${x + f}px;top:${y + f}px;width:20px;height:10px;border-radius:4px">`,
  },
  {
    id: "opacity",
    note: "square box, opacity 0.5",
    fg: 128,
    html: (x, y, f) =>
      `<div data-case style="left:${x + f}px;top:${y + f}px;width:30px;height:16px;background:#fff;opacity:0.5"></div>`,
  },
];

function fragment() {
  const style = `<style>
html { background: #000 !important; }
html, body { margin: 0; padding: 0; background: #000 !important; }
body > div, .bar { position: absolute; box-sizing: border-box; margin: 0; }
div { box-sizing: border-box; }
.static { position: static; }
.flex { display: flex; align-items: center; justify-content: center; }
.bar { border-bottom: 1px solid #000; }
.tabs { position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%); display: flex; align-items: center; gap: 2px; }
svg { display: block; }
@keyframes snap-hold { from { transform: translate(var(--f), var(--f)); } to { transform: translate(var(--f), var(--f)); } }
</style>`;
  const cells = [];
  CASES.forEach((c, row) => {
    OFFSETS.forEach((f, col) => {
      const x = LEFT + col * CELL_W;
      const y = TOP + row * CELL_H + 6;
      cells.push(c.html(x, y, f).replace("data-case", `data-case="${c.id}" data-f="${f}"`));
    });
  });
  return `${style}\n${cells.join("\n")}\n`;
}

const VIEWPORT = {
  width: LEFT * 2 + OFFSETS.length * CELL_W,
  height: TOP * 2 + CASES.length * CELL_H,
};

const MEASURE = `
(() => [...document.querySelectorAll("[data-case]")].map((e) => {
  const r = e.getBoundingClientRect();
  return { id: e.dataset.case, f: Number(e.dataset.f), rect: [r.left, r.top, r.right, r.bottom] };
}))()
`;

// The title bar of the booted app: boxes and SVG roots.
const TITLEBAR = `
(() => {
  const pick = (sel) => [...document.querySelectorAll(sel)].map((e) => {
    const r = e.getBoundingClientRect();
    return [r.left, r.top, r.right, r.bottom];
  });
  return {
    titlebar: pick(".titlebar"),
    tabs: pick(".tabs"),
    tab: pick(".tab"),
    tabSvg: pick(".tab svg"),
    btn: pick(".titlebar .btn"),
    btnSvg: pick(".titlebar .btn svg"),
    winBtn: pick(".win-btn"),
    winSvg: pick(".win-btn svg"),
  };
})()
`;

const APPLY_DEMO = `
(async () => {
  const { SCENARIOS } = await import("/src/lib/mock/scenarios.ts");
  const base = SCENARIOS.demo().stores;
  const settings = { ...(base["client.settings"] ?? {}), themeId: "glass-dark", language: "en" };
  localStorage.clear();
  sessionStorage.clear();
  localStorage.setItem("accshift.mock.overrides", JSON.stringify({
    scenario: "demo", runtimeOs: "windows", stores: { "client.settings": settings },
  }));
  return true;
})()
`;

/** Painted edges of one case along one axis. */
function edges(img, kind, fg, axis, lo, hi, line) {
  const at = (i) => {
    const [x, y] = axis === "h" ? [i, line] : [line, i];
    if (x < 0 || y < 0 || x >= img.width || y >= img.height) return 0;
    return Math.min(1, Math.max(0, img.data[(y * img.width + x) * 4 + 1] / fg));
  };
  // An offset band (a shadow) reaches past the box; the other kinds stay
  // within a few pixels, clear of the neighbouring rows.
  const s = Math.max(0, Math.floor(lo) - 4);
  const e = Math.ceil(hi) + (kind === "end" ? 16 : 4);
  const mid = Math.floor((lo + hi) / 2);
  if (kind === "solid") {
    let a = s;
    for (let i = s; i < mid; i++) a += 1 - at(i);
    let b = mid;
    for (let i = mid; i < e; i++) b += at(i);
    return { a: round2(a), b: round2(b), size: round2(b - a) };
  }
  const [from, to] = kind === "start" ? [s, mid] : [mid, e];
  let sum = 0;
  let moment = 0;
  for (let i = from; i < to; i++) {
    const c = at(i);
    sum += c;
    moment += (i + 0.5) * c;
  }
  const centre = sum ? moment / sum : NaN;
  return { a: round2(centre - sum / 2), b: round2(centre + sum / 2), size: round2(sum) };
}

/** RGBA pixels of a PNG, through ffmpeg (diff.mjs runs its CLI on import). */
function decodePng(file) {
  const buf = readFileSync(file);
  const width = buf.readUInt32BE(16);
  const height = buf.readUInt32BE(20);
  const data = execFileSync(
    process.env.FFMPEG ?? "ffmpeg",
    ["-v", "error", "-i", file, "-f", "rawvideo", "-pix_fmt", "rgba", "-"],
    { windowsHide: true, maxBuffer: 1 << 28 },
  );
  return { width, height, data };
}

const round2 = (v) => Math.round(v * 100) / 100;

function parseArgs(argv) {
  const opts = { out: null, exe: null, scales: [1, 1.25, 1.5, 1.75], dsf: "force" };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--out") opts.out = argv[++i];
    else if (argv[i] === "--exe") opts.exe = argv[++i];
    else if (argv[i] === "--scales") opts.scales = argv[++i].split(",").map(Number);
    else if (argv[i] === "--dsf") opts.dsf = argv[++i];
  }
  if (!opts.out) {
    console.error(
      "usage: measure.mjs --out <dir> [--exe <path>] [--scales 1,1.25,1.5,1.75] [--dsf emulate|force]",
    );
    process.exit(2);
  }
  return opts;
}

async function settle(js) {
  const started = Date.now();
  while (Date.now() - started < 30_000) {
    const ok = await js(
      `(() => !window.__parityStale && document.readyState === "complete" && !!document.querySelector(".titlebar .tab") && document.getAnimations().filter((a) => a.playState === "running" && a.effect?.getComputedTiming().endTime !== Infinity).length === 0)()`,
    ).catch(() => false);
    if (ok) return true;
    await sleep(100);
  }
  return false;
}

async function measureScale(page, scale, emulate, html, out, measures, titlebar) {
  const { send, js } = page;
  // Emulated: CDP scales a DSF 1 page. Forced: the real factor, no override.
  const dsf = emulate ? scale : 0;
  await send("Emulation.setDeviceMetricsOverride", {
    width: 860,
    height: 440,
    deviceScaleFactor: dsf,
    mobile: false,
  });
  await js(APPLY_DEMO);
  await js("window.__parityStale = true");
  await send("Page.navigate", { url: DEV_URL });
  if (!(await settle(js))) console.error(`scale ${scale}: title bar never settled`);
  titlebar[scale] = await js(TITLEBAR);
  titlebar[scale].devicePixelRatio = await js("window.devicePixelRatio");

  await send("Emulation.setDeviceMetricsOverride", {
    ...VIEWPORT,
    deviceScaleFactor: dsf,
    mobile: false,
  });
  await page.call((fragment) => {
    document.activeElement?.blur?.();
    document.body.innerHTML = fragment;
    return true;
  }, html);
  await js(
    "new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => r(true), 300))))",
  );
  const rects = await js(MEASURE);
  const { data } = await send("Page.captureScreenshot", { format: "png", fromSurface: true });
  const dir = join(out, String(scale));
  mkdirSync(dir, { recursive: true });
  const file = join(dir, "frame.png");
  writeFileSync(file, Buffer.from(data, "base64"));
  const img = decodePng(file);
  for (const m of rects) {
    const c = CASES.find((k) => k.id === m.id);
    const dev = m.rect.map((v) => v * scale);
    const cy = Math.floor((dev[1] + dev[3]) / 2);
    const cx = Math.floor((dev[0] + dev[2]) / 2);
    measures.push({
      id: m.id,
      scale,
      f: m.f,
      layout: dev.map(round2),
      h: edges(img, c.h ?? "solid", c.fg ?? 255, "h", dev[0], dev[2], cy),
      v: edges(img, c.v ?? "solid", c.fg ?? 255, "v", dev[1], dev[3], cx),
    });
  }
  console.log(`scale ${scale}: ${rects.length} cases measured, ${img.width}x${img.height}`);
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const out = resolve(opts.out);
  mkdirSync(out, { recursive: true });
  const html = fragment();
  writeFileSync(join(out, "snap.html"), html);
  const exe = resolve(opts.exe ?? defaultExe());
  const dirs = realAppDirs();
  const before = Object.fromEntries(dirs.map((d) => [d, snapshotTree(d)]));
  const regBefore = deepLinkRegistration();
  const watch = windowWatch();
  const vite = await startVite({ logFile: join(out, "vite.log") });
  let app = null;
  const measures = [];
  const titlebar = {};
  const prepare = async (page) => {
    await page.send("Page.enable");
    await page.send("Runtime.enable");
    await page.send("Page.addScriptToEvaluateOnNewDocument", { source: PIN_CLOCK });
  };
  try {
    if (opts.dsf === "emulate") {
      app = await launchApp({
        exe,
        scratch: join(out, ".scratch"),
        logFile: join(out, "app.log"),
        onStderr: watch.onLine,
        windowMode: "hidden",
      });
      await prepare(app.page);
      for (const scale of opts.scales) {
        await measureScale(app.page, scale, true, html, out, measures, titlebar);
      }
    } else {
      for (const scale of opts.scales) {
        app = await launchApp({
          exe,
          scratch: join(out, ".scratch"),
          logFile: join(out, `app-${scale}.log`),
          onStderr: watch.onLine,
          windowMode: "hidden",
          scale,
        });
        await prepare(app.page);
        await measureScale(app.page, scale, false, html, out, measures, titlebar);
        await app.close();
        app = null;
        await sleep(300);
      }
    }
  } finally {
    await app?.close();
    await vite.close();
  }
  await sleep(300);
  rmSync(join(out, ".scratch"), { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  const after = Object.fromEntries(dirs.map((d) => [d, snapshotTree(d)]));
  const touched = dirs.flatMap((d) => diffTrees(before[d], after[d]));
  const win = watch.summary();
  writeFileSync(join(out, "cases.json"), JSON.stringify(measures, null, 1) + "\n");
  writeFileSync(join(out, "titlebar.json"), JSON.stringify(titlebar, null, 1) + "\n");

  const md = [`# Snapping probe\n`];
  for (const c of CASES) {
    md.push(
      `## ${c.id}\n\n${c.note}. Device px; layout edges, then painted (h: left..right, v: top..bottom).\n`,
    );
    md.push("| scale | f | layout x | painted x | layout y | painted y |");
    md.push("| --- | --- | --- | --- | --- | --- |");
    for (const m of measures.filter((m) => m.id === c.id)) {
      md.push(
        `| ${m.scale} | ${m.f} | ${m.layout[0]}..${m.layout[2]} | ${m.h.a}..${m.h.b} (${m.h.size}) | ${m.layout[1]}..${m.layout[3]} | ${m.v.a}..${m.v.b} (${m.v.size}) |`,
      );
    }
    md.push("");
  }
  writeFileSync(join(out, "report.md"), md.join("\n"));
  console.log(
    `window: ${win.samples} samples -> hidden=${win.hidden}; real app dirs: ${touched.length ? `${touched.length} change(s)` : "untouched"}, accshift:// ${regBefore === deepLinkRegistration() ? "unchanged" : "CHANGED"}`,
  );
  console.log(`${measures.length} measures -> ${join(out, "report.md")}`);
  if (!win.hidden || touched.length) process.exitCode = 1;
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
