// Builds the installers of the app with the Tauri bundler (the Tauri CLI
// alone, no Tauri runtime) and ../tauri.conf.json: same product name,
// identifier, install dir, registry keys, shortcuts and updater artifacts as
// the v1.0.x Tauri builds, so installing over one is an ordinary update.
//
//   node crates/accshift-egui/bundle/bundle.mjs [--build] [--bundles <list>] [--no-updater]
//
// --build first builds the GUI and the CLI in release, with --locked.
// Without it both must already be built for the host. --bundles defaults to
// the installers of the host OS: nsis on Windows, appimage,deb,rpm on Linux,
// app,dmg on macOS. --no-updater skips the updater artifacts, which need
// TAURI_SIGNING_PRIVATE_KEY.
//
// The bundler packs `<target>/<triple>/release/accshift-gui[.exe]` when given
// a target triple: the egui binary is copied there under the name the
// installers and the updater know. The CLI goes next to it (`externalBin`)
// from `binaries/accshift-<triple>[.exe]`.
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, "..");
const root = join(crate, "..", "..");

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const option = (name) => {
  const at = args.indexOf(name);
  return at >= 0 ? args[at + 1] : undefined;
};

const DEFAULT_BUNDLES = { win32: "nsis", linux: "appimage,deb,rpm", darwin: "app,dmg" };
const bundles = option("--bundles") ?? DEFAULT_BUNDLES[process.platform];
if (!bundles) throw new Error(`no installer for ${process.platform}`);

function hostTriple() {
  const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  const match = out.match(/^host: (\S+)$/m);
  if (!match) throw new Error("could not parse the host triple from `rustc -vV`");
  return match[1];
}

// `<root>/target` is only the default: CARGO_TARGET_DIR or a cargo config
// can move it.
function targetDirectory() {
  const out = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    encoding: "utf8",
    cwd: root,
    maxBuffer: 32 * 1024 * 1024,
  });
  return JSON.parse(out).target_directory;
}

if (flag("--build")) {
  execFileSync(
    "cargo",
    ["build", "--release", "--locked", "-p", "accshift-egui", "-p", "accshift-cli"],
    { cwd: root, stdio: "inherit" },
  );
}

const triple = hostTriple();
const ext = process.platform === "win32" ? ".exe" : "";
const target = targetDirectory();
const built = join(target, "release");

const release = join(target, triple, "release");
mkdirSync(release, { recursive: true });
const gui = join(release, `accshift-gui${ext}`);
copyFileSync(join(built, `accshift-egui${ext}`), gui);
console.log(`staged GUI: ${gui}`);

const binaries = join(crate, "binaries");
mkdirSync(binaries, { recursive: true });
const cli = join(binaries, `accshift-${triple}${ext}`);
copyFileSync(join(built, `accshift${ext}`), cli);
console.log(`staged CLI: ${cli}`);

const configs = [];
if (flag("--no-updater")) {
  configs.push(JSON.stringify({ bundle: { createUpdaterArtifacts: false } }));
}

// The bundler downloads linuxdeploy's AppRun into its tool cache with mode
// 0770 and copies it into the AppDir as is, where linuxdeploy renames it
// AppRun.wrapped. When the kernel mounts the image instead of FUSE (firejail
// --appimage, the AppImage catalog test), a user other than root cannot run
// it: "AppRun.wrapped: Permission denied". The bundler only downloads a
// missing file, so a copy left there with mode 0755 is the one it packs.
const appimage = process.platform === "linux" && bundles.split(",").includes("appimage");
const toolsArch = /^arm.*hf$/.test(triple) ? "armhf" : triple.split("-")[0];

async function seedAppRun() {
  const cache = process.env.XDG_CACHE_HOME || join(homedir(), ".cache");
  const apprun = join(cache, "tauri", `AppRun-${toolsArch}`);
  if (!existsSync(apprun)) {
    const url = `https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-${toolsArch}`;
    const response = await fetch(url);
    if (!response.ok) throw new Error(`could not download ${url}: HTTP ${response.status}`);
    mkdirSync(dirname(apprun), { recursive: true });
    writeFileSync(apprun, Buffer.from(await response.arrayBuffer()));
  }
  chmodSync(apprun, 0o755);
  console.log(`seeded AppRun: ${apprun}`);
}

// Fails the build when the bundler stops packing the seeded copy (a new cache
// path or file name): every user must be able to run the image's entry points.
function checkAppImage(file) {
  const dir = mkdtempSync(join(tmpdir(), "accshift-appimage-"));
  try {
    execFileSync(file, ["--appimage-extract", "AppRun*"], { cwd: dir, stdio: "ignore" });
    for (const name of ["AppRun", "AppRun.wrapped"]) {
      const path = join(dir, "squashfs-root", name);
      if (!existsSync(path)) continue;
      const mode = statSync(path).mode & 0o777;
      if ((mode & 0o005) !== 0o005) {
        throw new Error(`${file}: ${name} has mode ${mode.toString(8)}, other users cannot run it`);
      }
    }
    console.log(`checked AppImage entry points: ${file}`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

if (appimage) await seedAppRun();

const tauri = createRequire(join(root, "package.json")).resolve("@tauri-apps/cli/tauri.js");
execFileSync(
  process.execPath,
  [
    tauri,
    "bundle",
    "--bundles",
    bundles,
    "--target",
    triple,
    ...configs.flatMap((config) => ["--config", config]),
  ],
  // The folder holding tauri.conf.json, found without a walk from the root.
  { cwd: root, stdio: "inherit", env: { ...process.env, TAURI_APP_PATH: crate } },
);

const out = join(release, "bundle");
for (const kind of readdirSync(out)) {
  for (const name of readdirSync(join(out, kind))) console.log(`bundle: ${join(out, kind, name)}`);
}

if (appimage) {
  const dir = join(out, "appimage");
  for (const name of readdirSync(dir).filter((name) => name.endsWith(".AppImage"))) {
    checkAppImage(join(dir, name));
  }
}
