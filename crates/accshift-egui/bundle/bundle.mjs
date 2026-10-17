// Builds the Windows installer of the egui app with the Tauri bundler and the
// same src-tauri/tauri.conf.json as the Tauri build: same product name,
// identifier, install dir, registry keys, shortcuts and updater artifacts, so
// installing it over v1.0.x is an ordinary update.
//
// The bundler packs `<target>/<triple>/release/accshift-gui.exe` when given a
// target triple, `<target>` being the root workspace's. The egui binary is
// copied there under that name: a plain build of the Tauri app writes
// `<target>/release`, so its accshift-gui.exe is never replaced, even in a
// target dir both share. bundle/tauri.conf.json drops the WebView2
// bootstrapper and swaps the installer hooks.
//
//   node crates/accshift-egui/bundle/bundle.mjs [--exe <accshift-egui.exe>] [--no-updater]
//
// --exe defaults to the release build of this crate. --no-updater skips the
// updater artifacts, which need TAURI_SIGNING_PRIVATE_KEY.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, "..");
const root = join(crate, "..", "..");

if (process.platform !== "win32") {
  throw new Error("only the Windows installer is wired so far");
}

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const option = (name) => {
  const at = args.indexOf(name);
  return at >= 0 ? args[at + 1] : undefined;
};

// This crate is a workspace of its own, so it builds into its own target dir
// unless CARGO_TARGET_DIR or a cargo config sends both to one place. Ask cargo
// for each, like scripts/stage-cli.mjs does for the root workspace.
function targetDirectory(cwd) {
  const out = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    encoding: "utf8",
    cwd,
    maxBuffer: 32 * 1024 * 1024,
  });
  return JSON.parse(out).target_directory;
}

const triple = "x86_64-pc-windows-msvc";
const exe = option("--exe") ?? join(targetDirectory(crate), "release", "accshift-egui.exe");
const release = join(targetDirectory(root), triple, "release");
mkdirSync(release, { recursive: true });
copyFileSync(exe, join(release, "accshift-gui.exe"));
console.log(`staged GUI: ${exe} -> ${join(release, "accshift-gui.exe")}`);

// The CLI sidecar (`externalBin`), from the root workspace's release build.
execFileSync(process.execPath, [join(root, "scripts", "stage-cli.mjs")], {
  cwd: root,
  stdio: "inherit",
  env: { ...process.env, TAURI_ENV_TARGET_TRIPLE: triple },
});

const configs = [join(here, "tauri.conf.json")];
if (flag("--no-updater")) {
  configs.push(JSON.stringify({ bundle: { createUpdaterArtifacts: false } }));
}

const tauri = createRequire(join(root, "package.json")).resolve("@tauri-apps/cli/tauri.js");
execFileSync(
  process.execPath,
  [
    tauri,
    "bundle",
    "--bundles",
    "nsis",
    "--target",
    triple,
    ...configs.flatMap((config) => ["--config", config]),
  ],
  { cwd: root, stdio: "inherit" },
);

const out = join(release, "bundle", "nsis");
for (const name of readdirSync(out)) console.log(`installer: ${join(out, name)}`);
