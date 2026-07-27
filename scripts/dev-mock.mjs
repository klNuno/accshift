// Starts the app with the MCP bridge AND a mock backend, so an agent (or the
// demo recorder) can click anything in the window without a chance of touching
// the real Steam, the keyring or the registry.
//
//   pnpm dev:mock            # "dev" scenario: edge cases, safe to change
//   pnpm dev:mock huge       # 200 accounts
//   pnpm dev:mock demo       # frozen dataset the README captures were shot on
//
// Scenarios live in src/lib/mock/scenarios.ts. Everything but `demo` can be
// re-pointed at runtime from the bridge without restarting this command:
//
//   window.__mock.state() / .use(id) / .accounts(n) / .os(name) /
//   .seed(store, value) / .latency(ms) / .fail(cmd, msg) / .reset()
//
// A plain `pnpm dev:mcp` still runs against the real backend — that is the one
// to use when testing an actual account switch.

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const scenario = process.argv[2] ?? "dev";
const extraArgs = process.argv.slice(3);

// Cheap parse rather than importing TypeScript: only used to catch a typo
// before a two-minute cargo build.
function knownScenarios() {
  try {
    const source = readFileSync(
      fileURLToPath(new URL("../src/lib/mock/scenarios.ts", import.meta.url)),
      "utf8",
    );
    const block = source.match(/SCENARIOS[^=]*=\s*\{([^}]*)\}/s);
    if (!block) return [];
    return [...block[1].matchAll(/^\s*(\w+):/gm)].map((match) => match[1]);
  } catch {
    return [];
  }
}

const known = knownScenarios();
if (known.length > 0 && !known.includes(scenario)) {
  console.error(`Unknown scenario "${scenario}". Known: ${known.join(", ")}`);
  process.exit(1);
}

const args = [
  "tauri",
  "dev",
  "--config",
  "src-tauri/tauri.mcp.conf.json",
  "--features",
  "mcp-bridge",
  ...extraArgs,
];

console.log(`[dev:mock] scenario "${scenario}" + MCP bridge`);

const child = spawn("pnpm", args, {
  stdio: "inherit",
  // Windows resolves pnpm through a shim, which spawn cannot exec directly.
  shell: true,
  env: { ...process.env, VITE_MOCK: scenario },
});

child.on("exit", (code, signal) => {
  process.exit(signal ? 1 : (code ?? 0));
});
