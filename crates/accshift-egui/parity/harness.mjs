// Shared plumbing of the parity bench: the vite dev server with the mock
// backend, the real Tauri app built with the `parity-capture` feature, and a
// raw CDP client on its own WebView2. Used by ref.mjs and dump-data.mjs.
//
// Nothing here opens a window or touches the real app data:
// - the app is a debug `accshift-gui` built with `--features parity-capture`
//   (checked through the marker string `accshift-parity-capture-build`), which
//   never shows its window (or, with --window cloak, shows it cloaked off
//   screen) and moves every app directory under ACCSHIFT_PARITY_DIR;
// - WEBVIEW2_USER_DATA_FOLDER points at the same scratch folder, and CDP is
//   opened by WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS on a free port;
// - the binary is copied into the scratch folder before launch, so another
//   build in the shared cargo target cannot swap it mid-run.

import { spawn, execFileSync } from "node:child_process";
import {
  copyFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO = resolve(HERE, "../../..");
/** `NOW` of src/lib/mock/fixtures.ts, in seconds. */
export const MOCK_NOW = 1_791_000_000;
/** `devUrl` of src-tauri/tauri.conf.json, compiled into the debug app. */
export const DEV_PORT = 1420;
export const DEV_URL = `http://localhost:${DEV_PORT}/`;
export const MARKER = "accshift-parity-capture-build";

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function freePort() {
  return new Promise((res, rej) => {
    const srv = createServer();
    srv.unref();
    srv.on("error", rej);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => res(port));
    });
  });
}

/** True when something already listens on 127.0.0.1:port. */
export function portBusy(port) {
  return new Promise((res) => {
    const srv = createServer();
    srv.once("error", () => res(true));
    srv.listen(port, "127.0.0.1", () => srv.close(() => res(false)));
  });
}

export function defaultExe() {
  const target = process.env.CARGO_TARGET_DIR ?? cargoTargetDir() ?? join(REPO, "target");
  return join(target, "debug", process.platform === "win32" ? "accshift-gui.exe" : "accshift-gui");
}

function cargoTargetDir() {
  try {
    const out = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
      cwd: REPO,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      windowsHide: true,
      maxBuffer: 64 << 20,
    });
    return JSON.parse(out).target_directory;
  } catch {
    return null;
  }
}

/** Refuse any binary that is not a parity-capture build. */
export function assertParityBuild(exe) {
  if (!existsSync(exe))
    throw new Error(
      `no binary at ${exe}; build it with: cargo build -p accshift-gui --features parity-capture`,
    );
  if (!readFileSync(exe).includes(Buffer.from(MARKER))) {
    throw new Error(`${exe} is not a parity-capture build (marker ${MARKER} missing)`);
  }
}

function killTree(pid) {
  if (!pid) return;
  try {
    if (process.platform === "win32") {
      execFileSync("taskkill", ["/PID", String(pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
      });
    } else {
      process.kill(-pid, "SIGKILL");
    }
  } catch {
    /* already gone */
  }
}

async function waitHttp(url, timeoutMs, child) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (child?.exitCode != null) throw new Error(`${url}: process exited with ${child.exitCode}`);
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch {
      /* not up yet */
    }
    await sleep(150);
  }
  throw new Error(`${url} did not answer within ${timeoutMs} ms`);
}

/**
 * The frontend on the vite dev server, non-frozen mock (`VITE_MOCK=dev`) so
 * the scene can pick any scenario through `accshift.mock.overrides`.
 */
export async function startVite({ logFile }) {
  if (await portBusy(DEV_PORT)) {
    throw new Error(
      `port ${DEV_PORT} is taken: the debug app loads its devUrl from there, stop whatever owns it`,
    );
  }
  const vp = join(REPO, "node_modules", "vite-plus", "bin", "vp");
  const log = createWriteStream(logFile);
  const child = spawn(process.execPath, [vp, "dev", "--port", String(DEV_PORT), "--strictPort"], {
    cwd: REPO,
    env: { ...process.env, VITE_MOCK: "dev", BROWSER: "none" },
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.stdout.pipe(log);
  child.stderr.pipe(log);
  await waitHttp(DEV_URL, 60_000, child);
  return {
    child,
    close: async () => {
      killTree(child.pid);
      log.end();
    },
  };
}

/**
 * Launch the parity build against the dev server. `scratch` receives the app
 * directories (ACCSHIFT_PARITY_DIR), the WebView2 profile and the binary copy.
 * `onStderr` sees every stderr line (the `parity-window` samples among them).
 *
 * `scale` (optional) is Chromium's real device scale factor, through
 * `--force-device-scale-factor`: what a Windows display at that scaling
 * gives. Layout then runs in device pixels (border widths floored to whole
 * device pixels, boxes snapped to the device grid). The CDP emulation
 * (`Emulation.setDeviceMetricsOverride` with a `deviceScaleFactor`) is not
 * the same thing: it lays the page out at the window's own factor and scales
 * the frame, so 1 CSS px of border stays 1.5 device px at 150 %.
 */
export async function launchApp({ exe, scratch, logFile, onStderr, windowMode, scale }) {
  assertParityBuild(exe);
  rmSync(scratch, { recursive: true, force: true });
  mkdirSync(join(scratch, "bin"), { recursive: true });
  const copy = join(scratch, "bin", exe.split(/[\\/]/).pop());
  copyFileSync(exe, copy);
  assertParityBuild(copy);

  const cdpPort = await freePort();
  const log = createWriteStream(logFile);
  const env = {
    ...process.env,
    ACCSHIFT_PARITY_DIR: join(scratch, "app"),
    ACCSHIFT_PARITY_WINDOW: windowMode ?? "hidden",
    WEBVIEW2_USER_DATA_FOLDER: join(scratch, "app", "webview"),
    // Replaces wry's own argument list, hence its three disabled features
    // repeated here. CalculateNativeWinOcclusion off: Chromium would read the
    // cloaked, off-screen window as occluded and stop painting.
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: [
      `--remote-debugging-port=${cdpPort}`,
      ...(scale ? [`--force-device-scale-factor=${scale}`] : []),
      "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion",
      "--disable-backgrounding-occluded-windows",
      "--disable-renderer-backgrounding",
    ].join(" "),
  };
  const child = spawn(copy, [], {
    cwd: scratch,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.stdout.pipe(log);
  let pending = "";
  child.stderr.on("data", (chunk) => {
    log.write(chunk);
    pending += chunk.toString("utf8");
    const lines = pending.split(/\r?\n/);
    pending = lines.pop();
    for (const line of lines) onStderr?.(line);
  });

  // The page target exists once WebView2 is up and the app navigated to its
  // devUrl.
  const started = Date.now();
  let target = null;
  while (!target && Date.now() - started < 60_000) {
    if (child.exitCode != null)
      throw new Error(`app exited with ${child.exitCode}, see ${logFile}`);
    try {
      const list = await (await fetch(`http://127.0.0.1:${cdpPort}/json`)).json();
      target = list.find((t) => t.type === "page" && t.url.startsWith(DEV_URL));
    } catch {
      /* CDP not up yet */
    }
    if (!target) await sleep(200);
  }
  if (!target) throw new Error(`no page on ${DEV_URL} over CDP ${cdpPort}, see ${logFile}`);
  const page = await connect(target.webSocketDebuggerUrl);
  // The target can be listed before its document commits; localStorage
  // throws on anything but the dev origin.
  const origin = DEV_URL.slice(0, -1);
  while (Date.now() - started < 60_000) {
    const ready = await page
      .js(`location.origin === ${JSON.stringify(origin)} && document.readyState === "complete"`)
      .catch(() => false);
    if (ready) break;
    await sleep(200);
  }
  return {
    child,
    cdpPort,
    page,
    close: async () => {
      page.close();
      killTree(child.pid);
      await new Promise((r) => (child.exitCode != null ? r() : child.once("exit", r)));
      log.end();
    },
  };
}

export async function connect(wsUrl) {
  const ws = new WebSocket(wsUrl);
  await new Promise((res, rej) => {
    ws.onopen = res;
    ws.onerror = rej;
  });
  let seq = 0;
  const pending = new Map();
  ws.onmessage = (event) => {
    const msg = JSON.parse(event.data);
    if (msg.id && pending.has(msg.id)) {
      const { res, rej } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) rej(new Error(JSON.stringify(msg.error)));
      else res(msg.result);
    }
  };
  const send = (method, params = {}) =>
    new Promise((res, rej) => {
      const id = ++seq;
      pending.set(id, { res, rej });
      ws.send(JSON.stringify({ id, method, params }));
    });
  const js = async (expression) => {
    const r = await send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (r.exceptionDetails) {
      throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    }
    return r.result.value;
  };
  // `fn` runs in the page with `args` passed as values: data never becomes
  // source text.
  const call = async (fn, ...args) => {
    const { result: global } = await send("Runtime.evaluate", { expression: "globalThis" });
    const r = await send("Runtime.callFunctionOn", {
      objectId: global.objectId,
      functionDeclaration: fn.toString(),
      arguments: args.map((value) => ({ value })),
      awaitPromise: true,
      returnByValue: true,
    });
    if (r.exceptionDetails) {
      throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    }
    return r.result.value;
  };
  return { send, js, call, close: () => ws.close() };
}

/**
 * `Date.now()` and argument-less `new Date()` start at the mock's NOW when the
 * document starts and advance with `performance.now()`, so relative labels
 * ("2 hours ago") never drift while timers and debounces still see time pass.
 * `Date()` called as a function keeps returning a string.
 */
export const PIN_CLOCK = `
(() => {
  const NOW_MS = ${MOCK_NOW * 1000};
  const RealDate = Date;
  const now = () => NOW_MS + Math.floor(performance.now());
  class PinnedDate extends RealDate {
    constructor(...args) {
      if (args.length === 0) super(now());
      else super(...args);
    }
    static now() { return now(); }
  }
  globalThis.Date = new Proxy(PinnedDate, {
    apply() { return new PinnedDate().toString(); },
  });
})();
`;

/** Every file under `dir` with its mtime and size, keyed by relative path. */
export function snapshotTree(dir) {
  const out = {};
  const walk = (d) => {
    let entries;
    try {
      entries = readdirSync(d, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      const p = join(d, e.name);
      if (e.isDirectory()) walk(p);
      else {
        try {
          const s = statSync(p);
          out[p.slice(dir.length + 1)] = `${s.mtimeMs}:${s.size}`;
        } catch {
          /* vanished */
        }
      }
    }
  };
  walk(dir);
  return out;
}

export function diffTrees(before, after) {
  const changed = [];
  for (const [k, v] of Object.entries(after)) {
    if (!(k in before)) changed.push(`added ${k}`);
    else if (before[k] !== v) changed.push(`changed ${k}`);
  }
  for (const k of Object.keys(before)) if (!(k in after)) changed.push(`removed ${k}`);
  return changed;
}

/** The real app's directories, which a parity run must leave untouched. */
export function realAppDirs() {
  if (process.platform !== "win32") return [];
  const id = "com.accshift.desktop";
  return [process.env.APPDATA, process.env.LOCALAPPDATA]
    .filter(Boolean)
    .map((base) => join(base, id));
}

/** The accshift:// handler in HKCU, which `register_all()` would rewrite. */
export function deepLinkRegistration() {
  if (process.platform !== "win32") return "";
  try {
    return execFileSync(
      "reg",
      ["query", "HKCU\\Software\\Classes\\accshift\\shell\\open\\command"],
      {
        encoding: "utf8",
        windowsHide: true,
        stdio: ["ignore", "pipe", "ignore"],
      },
    ).trim();
  } catch {
    return "(absent)";
  }
}

/** Whether the installed app runs, which would explain changes in its dirs. */
export function installedAppRunning() {
  if (process.platform !== "win32") return false;
  try {
    const out = execFileSync(
      "tasklist",
      ["/FI", "IMAGENAME eq accshift-gui.exe", "/FO", "CSV", "/NH", "/V"],
      {
        encoding: "utf8",
        windowsHide: true,
      },
    );
    return out.includes("accshift-gui.exe");
  } catch {
    return false;
  }
}

/**
 * Proof the window never reached the screen, from the `parity-window` samples
 * the app prints on stderr: every sample cloaked or invisible, off every
 * monitor, never foreground.
 */
export function windowWatch() {
  const samples = [];
  const onLine = (line) => {
    const m = line.match(
      /^parity-window (\w+) visible=(\w+) cloaked=(\d+) foreground=(\w+) rect=(-?\d+),(-?\d+),(-?\d+),(-?\d+)/,
    );
    if (!m) return;
    samples.push({
      when: m[1],
      visible: m[2] === "true",
      cloaked: Number(m[3]),
      foreground: m[4] === "true",
      rect: [Number(m[5]), Number(m[6]), Number(m[7]), Number(m[8])],
    });
  };
  const summary = () => {
    const seen = samples.filter(
      (s) =>
        s.foreground || (s.visible && s.cloaked === 0 && s.rect[2] > -30000 && s.rect[3] > -30000),
    );
    const first = samples.find((s) => s.when === "shown") ?? samples[0];
    return {
      samples: samples.length,
      everVisible: samples.some((s) => s.visible),
      everUncloaked: samples.some((s) => s.visible && s.cloaked === 0),
      everForeground: samples.some((s) => s.foreground),
      firstRect: first?.rect ?? null,
      hidden: samples.length > 0 && seen.length === 0,
    };
  };
  return { onLine, samples, summary };
}
