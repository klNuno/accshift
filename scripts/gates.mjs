// Runs every pre-commit gate in one command and reports each exit code.
//
//   pnpm gates                 all gates
//   pnpm gates rust worker     only the named gates
//   pnpm gates --skip rust     all but the named gates
//
// Each gate runs to completion even after a failure, so one run shows every
// broken gate. The process exits non-zero when any gate failed. Nothing is
// piped: a gate's own exit code is the one reported.
//
// Cargo gets CARGO_BUILD_JOBS=4 unless the caller already set a value. The
// Worker type-check calls the compiler directly: `pnpm --dir server exec` can
// try to reinstall `server/node_modules` and die on
// ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY.
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const node = JSON.stringify(process.execPath);

const GATES = [
  { name: "check", cmd: "pnpm run check" },
  { name: "vitest", cmd: `${node} node_modules/vitest/vitest.mjs run --maxWorkers=4` },
  { name: "worker", cmd: `${node} node_modules/typescript/bin/tsc --noEmit`, cwd: "server" },
  { name: "fmt", cmd: "pnpm run fmt:check" },
  { name: "clippy", cmd: "pnpm run clippy" },
  { name: "rust", cmd: "pnpm run test:rust" },
];

function select(argv) {
  const known = new Set(GATES.map((g) => g.name));
  const skip = argv[0] === "--skip";
  const names = skip ? argv.slice(1) : argv;
  for (const name of names) {
    if (!known.has(name)) {
      console.error(`unknown gate "${name}", known: ${[...known].join(", ")}`);
      process.exit(2);
    }
  }
  if (names.length === 0) return GATES;
  return GATES.filter((g) => names.includes(g.name) !== skip);
}

const env = { ...process.env, CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS || "4" };
const results = [];

for (const gate of select(process.argv.slice(2))) {
  console.log(`\n=== ${gate.name}: ${gate.cmd}`);
  const started = Date.now();
  const run = spawnSync(gate.cmd, {
    cwd: join(root, gate.cwd ?? ""),
    env,
    shell: true,
    stdio: "inherit",
  });
  const code = run.error ? `spawn error: ${run.error.message}` : run.status;
  results.push({ name: gate.name, code, seconds: ((Date.now() - started) / 1000).toFixed(1) });
}

console.log("\n=== summary");
for (const r of results) {
  console.log(
    `${r.code === 0 ? "ok  " : "FAIL"} ${r.name.padEnd(7)} exit ${r.code} in ${r.seconds}s`,
  );
}
process.exit(results.every((r) => r.code === 0) ? 0 : 1);
