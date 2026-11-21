// Builds the plugin, validates it and packs dist/dev.mtsu.accshift.streamDeckPlugin
// with the Elgato CLI (`streamdeck validate`, then `streamdeck pack`).
//
//   node scripts/package.mjs
//
// Schemas are not refreshed from the network (--no-update-check): the
// result depends only on the locked @elgato/cli.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { buildPlugin, PLUGIN_DIR, ROOT } from "./build.mjs";

const require = createRequire(join(ROOT, "package.json"));
const cli = join(require.resolve("@elgato/cli/package.json"), "..", "bin", "streamdeck.mjs");
const dist = join(ROOT, "dist");

function streamdeck(...args) {
  console.log(`\n> streamdeck ${args.join(" ")}`);
  const run = spawnSync(process.execPath, [cli, ...args], { cwd: ROOT, stdio: "inherit" });
  if (run.status !== 0) {
    console.error(`streamdeck ${args[0]} failed (exit ${run.status})`);
    process.exit(run.status ?? 1);
  }
}

await buildPlugin();
streamdeck("validate", PLUGIN_DIR, "--no-update-check");
mkdirSync(dist, { recursive: true });
streamdeck("pack", PLUGIN_DIR, "--output", dist, "--force", "--no-update-check");

const file = join(dist, "dev.mtsu.accshift.streamDeckPlugin");
if (!existsSync(file)) {
  console.error(`expected ${file} after packing`);
  process.exit(1);
}
console.log(`\npackaged ${file}`);
