// Stages the CLI binary where the Tauri bundler expects external binaries.
//
// `externalBin: ["binaries/accshift"]` makes the bundler look for
// `src-tauri/binaries/accshift-<target-triple>[.exe]` and ship it next to the
// GUI binary (NSIS install dir, /usr/bin for deb/rpm, Contents/MacOS for the
// .app). This script copies the freshly built CLI from the workspace target
// dir to that location. Runs as part of `beforeBuildCommand`.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function hostTriple() {
  const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  const match = out.match(/^host: (\S+)$/m);
  if (!match) throw new Error("could not parse host triple from `rustc -vV`");
  return match[1];
}

// Tauri sets TAURI_ENV_TARGET_TRIPLE for build hooks; fall back to the rustc
// host triple for manual invocations. Both match as long as neither the CLI
// build nor `tauri build` cross-compiles.
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || hostTriple();
const ext = triple.includes("windows") ? ".exe" : "";

const src = join(root, "target", "release", `accshift${ext}`);
const destDir = join(root, "src-tauri", "binaries");
const dest = join(destDir, `accshift-${triple}${ext}`);

mkdirSync(destDir, { recursive: true });
copyFileSync(src, dest);
console.log(`staged CLI: ${src} -> ${dest}`);
