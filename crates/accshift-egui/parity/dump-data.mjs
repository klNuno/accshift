// Mock data for the egui side of the parity bench: per scenario, the resolved
// result of every mocked command, read from the same fixtures module the
// reference app runs on.
//
//   node crates/accshift-egui/parity/dump-data.mjs [scenario ...] [--exe <path>]
//
// No scenario argument dumps them all. ACCSHIFT_PARITY_SCRATCH moves the
// run's scratch folder (default: the system temp dir). Writes
// crates/accshift-egui/parity/data/<scenario>.json. Needs the parity-capture
// build of accshift-gui, like ref.mjs: the modules are imported through the
// vite dev server from inside its WebView2 (Runtime.evaluate), so they resolve
// exactly as in the app.
//
// Format (version 1):
//
//   {
//     "format": 1,
//     "scenario": "demo",
//     "now": 1791000000,              // mock NOW, seconds
//     "spec": {                       // scenario fields no command returns
//       "label", "runtimeOs", "switchDelayMs", "steamPath", "hasSteamApiKey",
//       "currentSteamAccount", "currentRiotProfile", "currentRobloxAccount"
//     },
//     "commands": {
//       "<command>": [
//         { "args": { ... }, "result": <value> }   // or "error": "<message>"
//       ]
//     },
//     "skipped": { "<command>": "<why>" }
//   }
//
// Each command is called once with `{}` unless it reads arguments: the
// platform commands once per platform id ("steam", "riot", "roblox"), the
// per-account ones once per account, the batch ones once with every id.
// Cache timestamps that the fixtures take from `Date.now()` are taken at
// exactly NOW * 1000. Avatar URLs (`http://localhost:1420/src/lib/mock/
// avatars/<n>.svg`) are rewritten to repo-relative paths
// (`src/lib/mock/avatars/<n>.svg`).

import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { HERE, MOCK_NOW, REPO, defaultExe, launchApp, startVite } from "./harness.mjs";

const SKIP = {
  platform_switch_account: "changes the current account after switchDelayMs; modelled from spec",
  pin_unlock: "PIN session state, reads the live settings store",
  pin_lock: "PIN session state",
};

const DUMP = (scenario) => `
(async () => {
  const { SCENARIOS } = await import("/src/lib/mock/scenarios.ts");
  const { createHandlers, NOW } = await import("/src/lib/mock/fixtures.ts");
  const id = ${JSON.stringify(scenario)};
  const skip = ${JSON.stringify(SKIP)};
  const realNow = Date.now;
  Date.now = () => NOW * 1000;
  try {
    const spec = SCENARIOS[id]();
    const handlers = createHandlers(spec);
    const platforms = ["steam", "riot", "roblox"];
    const steamIds = spec.steamAccounts.map((a) => a.steam_id);
    const robloxIds = spec.robloxAccounts.map((a) => a.userId);
    const argSets = {
      platform_get_accounts: platforms.map((platformId) => ({ platformId })),
      platform_get_current_account: platforms.map((platformId) => ({ platformId })),
      platform_get_startup_snapshot: platforms.map((platformId) => ({ platformId })),
      platform_dry_run: steamIds.slice(0, 1).map((accountId) => ({ platformId: "steam", accountId })),
      steam_get_profile_info: steamIds.map((accountId) => ({ accountId })),
      steam_get_profile_infos: [{ accountIds: steamIds }],
      steam_get_player_bans: [{ steamIds }],
      roblox_get_profile_info: robloxIds.map((userId) => ({ userId })),
    };
    const commands = {};
    const skipped = {};
    for (const name of Object.keys(handlers).sort()) {
      if (skip[name]) {
        skipped[name] = skip[name];
        continue;
      }
      commands[name] = [];
      for (const args of argSets[name] ?? [{}]) {
        try {
          commands[name].push({ args, result: (await handlers[name](args)) ?? null });
        } catch (error) {
          commands[name].push({ args, error: String(error?.message ?? error) });
        }
      }
    }
    return {
      format: 1,
      scenario: id,
      now: NOW,
      spec: {
        label: spec.label,
        runtimeOs: spec.runtimeOs,
        switchDelayMs: spec.switchDelayMs,
        steamPath: spec.steamPath,
        hasSteamApiKey: spec.hasSteamApiKey,
        currentSteamAccount: spec.currentSteamAccount,
        currentRiotProfile: spec.currentRiotProfile,
        currentRobloxAccount: spec.currentRobloxAccount,
      },
      commands,
      skipped,
    };
  } finally {
    Date.now = realNow;
  }
})()
`;

// http://localhost:1420/src/lib/mock/avatars/03.svg?no-inline -> src/lib/mock/avatars/03.svg
function relink(value) {
  if (typeof value === "string") {
    const m = value.match(
      /^https?:\/\/(?:localhost|127\.0\.0\.1)(?::\d+)?\/(src\/lib\/mock\/avatars\/[^?#]+)/,
    );
    return m ? m[1] : value;
  }
  if (Array.isArray(value)) return value.map(relink);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, relink(v)]));
  }
  return value;
}

async function main() {
  const argv = process.argv.slice(2);
  let exe = defaultExe();
  const wanted = [];
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--exe") exe = argv[++i];
    else wanted.push(argv[i]);
  }
  const outDir = join(HERE, "data");
  // App dirs, WebView2 profile and logs of the run; removed at the end.
  const scratch = resolve(
    process.env.ACCSHIFT_PARITY_SCRATCH ?? join(tmpdir(), "accshift-parity-dump"),
  );
  mkdirSync(outDir, { recursive: true });
  mkdirSync(scratch, { recursive: true });

  const vite = await startVite({ logFile: join(scratch, "dump-vite.log") });
  let app = null;
  try {
    app = await launchApp({
      exe: resolve(exe),
      scratch: join(scratch, "dump"),
      logFile: join(scratch, "dump-app.log"),
    });
    const { js } = app.page;
    const ids = wanted.length
      ? wanted
      : await js(`import("/src/lib/mock/scenarios.ts").then((m) => Object.keys(m.SCENARIOS))`);
    for (const id of ids) {
      const data = relink(await js(DUMP(id)));
      if (data.now !== MOCK_NOW)
        console.warn(`${id}: fixtures NOW ${data.now} differs from harness ${MOCK_NOW}`);
      const file = join(outDir, `${id}.json`);
      writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
      const calls = Object.values(data.commands).reduce((n, c) => n + c.length, 0);
      console.log(
        `${id}: ${Object.keys(data.commands).length} commands, ${calls} calls -> ${file}`,
      );
    }
  } finally {
    await app?.close();
    await vite.close();
  }
  // The repo formatter checks every tracked file: hand it the dumps now
  // rather than fail `pnpm check` later.
  execFileSync(
    process.execPath,
    [join(REPO, "node_modules", "vite-plus", "bin", "vp"), "fmt", outDir],
    {
      cwd: REPO,
      stdio: "ignore",
      windowsHide: true,
    },
  );
  rmSync(scratch, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
