// Real Chromium proof for src/legacy_web_storage: the parity-capture build of
// the Tauri app (WebView2, window never shown, profile and app dirs in a
// scratch folder) gets the 12 legacy localStorage keys over CDP, then is
// closed cleanly or killed. The LevelDB WebView2 leaves behind is copied here
// for tests/legacy_web_storage.rs (`real_chromium_*`).
//
//   cargo build -p accshift-gui --features parity-capture
//   node crates/accshift-app/tests/fixtures/legacy_web_storage/chromium-proof.mjs \
//     --scratch <dir> [--out <dir>] [--exe <path>]
//
// --out defaults to this folder. Writes <out>/expected.json and, per case,
// <out>/<case>/ (the LevelDB files, LOCK and the text LOGs left out), and
// <scratch>/run.json (timings, window samples, real-dir check). Cases:
// - clean: phase 1, 6 s for Chromium's commit delay, phase 2, 2 s, then the
//   titlebar close (`close_window`): the app exits with its last window and
//   WebView2 shuts down on its own.
// - kill: same phases, then `taskkill /T /F` 1 s after phase 2.
// Phase 1 sets the 12 keys; phase 2 overwrites one and removes another.
// The page origin is the devUrl, http://localhost:1420.

import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  DEV_PORT,
  defaultExe,
  diffTrees,
  launchApp,
  portBusy,
  realAppDirs,
  sleep,
  snapshotTree,
  startVite,
  windowWatch,
} from "../../../../accshift-egui/parity/harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));

function args(argv) {
  const o = { out: HERE, scratch: null, exe: null };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--out") o.out = argv[++i];
    else if (argv[i] === "--scratch") o.scratch = argv[++i];
    else if (argv[i] === "--exe") o.exe = argv[++i];
    else throw new Error(`unknown argument ${argv[i]}`);
  }
  if (!o.scratch) throw new Error("--scratch <dir> is required");
  return o;
}

/** Deterministic values: one over 64 KB, Latin-1, non-Latin-1, pure ASCII. */
function values() {
  const avatars = {};
  for (let i = 0; i < 600; i++) {
    const id = String(76561198000000000n + BigInt(i * 7919));
    avatars[id] = {
      url: `https://avatars.steamstatic.com/${((i * 2654435761) >>> 0).toString(16).padStart(8, "0")}${"0".repeat(32)}_full.jpg`,
      fetchedAt: 1_790_000_000_000 + i * 1000,
    };
  }
  const phase1 = {
    // Pure ASCII.
    accshift_settings: JSON.stringify({ themeId: "dark", language: "fr", animations: "full" }),
    // Latin-1, not ASCII: Chromium stores it with the 0x01 format byte.
    accshift_folders: JSON.stringify({
      version: 1,
      folders: [{ id: "f1", name: "Dossier é à ç ñ ü" }],
      itemOrder: {},
    }),
    accshift_personas: JSON.stringify([{ id: "p1", name: "Persona", accent: "#8b5cf6" }]),
    // Beyond Latin-1, astral plane included: 0x00 format byte (UTF-16LE).
    accshift_account_card_notes: JSON.stringify({
      "steam:76561198000000001": "Compte principal ✓ 日本語 Ωμέγα 😀",
    }),
    accshift_account_card_colors: JSON.stringify({ "steam:76561198000000001": "violet" }),
    accshift_account_default_game: JSON.stringify({ "steam:76561198000000001": 730 }),
    accshift_folder_card_colors: JSON.stringify({ f1: "blue" }),
    accshift_viewMode: "list",
    // Over 64 KB.
    accshift_avatars: JSON.stringify(avatars),
    accshift_roblox_avatars: JSON.stringify({
      1234567: { url: "https://tr.rbxcdn.com/x/150/150/AvatarHeadshot/Png" },
    }),
    accshift_ban_check_state_v2: JSON.stringify({ lastCheck: 1_790_000_000_000, queue: [] }),
    accshift_ban_info_cache_v1: JSON.stringify({ "76561198000000001": { vac: 0, game: 0 } }),
  };
  const phase2 = {
    set: {
      accshift_ban_info_cache_v1: JSON.stringify({
        "76561198000000001": { vac: 1, game: 2, note: "réécrit ✓" },
      }),
    },
    remove: ["accshift_ban_check_state_v2"],
  };
  const final = { ...phase1, ...phase2.set };
  for (const k of phase2.remove) delete final[k];
  return { phase1, phase2, final };
}

function findLeveldb(root) {
  const stack = [root];
  while (stack.length) {
    const dir = stack.pop();
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const e of entries) {
      if (!e.isDirectory()) continue;
      const p = join(dir, e.name);
      if (e.name === "leveldb" && dir.endsWith("Local Storage")) return p;
      stack.push(p);
    }
  }
  return null;
}

/** WebView2 processes still using `profile` (their command line names it). */
function webviewPids(profile) {
  const ps = `Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like '*${profile.replace(/'/g, "''")}*' } | ForEach-Object { $_.ProcessId }`;
  const out = execFileSync("pwsh", ["-NoProfile", "-NonInteractive", "-Command", ps], {
    encoding: "utf8",
    windowsHide: true,
  });
  return out.split(/\s+/).filter(Boolean).map(Number);
}

async function waitGone(pid, profile, timeoutMs) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    let alive = true;
    try {
      process.kill(pid, 0);
    } catch {
      alive = false;
    }
    if (!alive && webviewPids(profile).length === 0) return Date.now() - start;
    await sleep(250);
  }
  return null;
}

async function runCase(name, mode, opts, vals, watch) {
  const scratch = join(opts.scratch, name);
  const app = await launchApp({
    exe: opts.exe,
    scratch,
    logFile: join(opts.scratch, `${name}-app.log`),
    onStderr: watch.onLine,
    windowMode: "hidden",
  });
  const profile = join(scratch, "app", "webview");
  const info = { case: name, mode };
  let closed = false;
  try {
    const { js, call } = app.page;
    info.origin = await js("location.origin");
    info.phase1Length = await call((v) => {
      for (const [k, x] of Object.entries(v)) localStorage.setItem(k, x);
      return Object.keys(v).filter((k) => localStorage.getItem(k) === v[k]).length;
    }, vals.phase1);
    await sleep(6000);
    info.phase2Ok = await call((p) => {
      for (const [k, x] of Object.entries(p.set)) localStorage.setItem(k, x);
      for (const k of p.remove) localStorage.removeItem(k);
      return (
        Object.entries(p.set).every(([k, x]) => localStorage.getItem(k) === x) &&
        p.remove.every((k) => localStorage.getItem(k) === null)
      );
    }, vals.phase2);
    if (mode === "clean") {
      await sleep(2000);
      // The titlebar's close button (`TitleBar.svelte`, `close_window`):
      // the window closes, the app exits once its last window is gone and
      // WebView2 shuts down on its own. `taskkill` without /F does not reach
      // the hidden window.
      await app.page.js(`void window.__TAURI_INTERNALS__.invoke("close_window"); true`);
      app.page.close();
      info.exitAfterMs = await waitGone(app.child.pid, profile, 30_000);
      info.exitCode = app.child.exitCode;
      closed = info.exitAfterMs != null;
    } else {
      app.page.close();
      await sleep(1000);
      await app.close();
      info.exitAfterMs = await waitGone(app.child.pid, profile, 15_000);
      closed = true;
    }
  } finally {
    if (!closed) {
      info.forcedCleanup = webviewPids(profile);
      await app.close();
      for (const pid of webviewPids(profile)) {
        try {
          execFileSync("taskkill", ["/PID", String(pid), "/F"], {
            stdio: "ignore",
            windowsHide: true,
          });
        } catch {
          /* gone */
        }
      }
    }
  }
  const leveldb = findLeveldb(profile);
  if (!leveldb) throw new Error(`${name}: no Local Storage/leveldb under ${profile}`);
  const dest = join(opts.out, name);
  rmSync(dest, { recursive: true, force: true });
  mkdirSync(dest, { recursive: true });
  info.files = {};
  for (const f of readdirSync(leveldb)) {
    if (["LOCK", "LOG", "LOG.old"].includes(f)) continue;
    cpSync(join(leveldb, f), join(dest, f));
    info.files[f] = statSync(join(dest, f)).size;
  }
  return info;
}

async function main() {
  const opts = args(process.argv.slice(2));
  opts.out = resolve(opts.out);
  opts.scratch = resolve(opts.scratch);
  opts.exe = resolve(opts.exe ?? defaultExe());
  mkdirSync(opts.scratch, { recursive: true });
  const vals = values();
  const dirs = realAppDirs();
  const before = Object.fromEntries(dirs.map((d) => [d, snapshotTree(d)]));
  const watch = windowWatch();
  // Other parity runs share port 1420: wait for it, never stop their server.
  let vite = null;
  for (const start = Date.now(); !vite;) {
    if (!(await portBusy(DEV_PORT))) {
      try {
        vite = await startVite({ logFile: join(opts.scratch, "vite.log") });
        break;
      } catch (err) {
        if (!(await portBusy(DEV_PORT))) throw err;
      }
    }
    if (Date.now() - start > 30 * 60_000) throw new Error(`port ${DEV_PORT} busy for 30 min`);
    console.error(`port ${DEV_PORT} busy, retrying in 15 s`);
    await sleep(15_000);
  }
  const cases = [];
  try {
    cases.push(await runCase("clean", "clean", opts, vals, watch));
    cases.push(await runCase("kill", "kill", opts, vals, watch));
  } finally {
    await vite.close();
  }
  await sleep(300);
  const touched = dirs.flatMap((d) =>
    diffTrees(before[d], snapshotTree(d)).map((c) => `${d}: ${c}`),
  );
  const win = watch.summary();
  writeFileSync(
    join(opts.out, "expected.json"),
    JSON.stringify(
      { origin: "http://localhost:1420", phase1: vals.phase1, final: vals.final },
      null,
      2,
    ) + "\n",
  );
  const run = {
    cases,
    window: { samples: win.samples, hidden: win.hidden },
    realDirsTouched: touched,
    port1420FreeAfter: !(await portBusy(DEV_PORT)),
  };
  writeFileSync(join(opts.scratch, "run.json"), JSON.stringify(run, null, 2) + "\n");
  console.log(JSON.stringify(run, null, 2));
  if (!win.hidden || touched.length) process.exitCode = 1;
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
