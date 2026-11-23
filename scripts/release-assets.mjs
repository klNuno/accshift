// The release downloads under names without a version, so a link to
// releases/latest/download/<name> (README, docs) keeps working at each release.
//
//   node scripts/release-assets.mjs <dir>
//
// Renames each bundle in <dir> and its updater .sig to its stable name, then
// fails when a download is missing: a release without it would break those
// links. The bundler and the build jobs still write versioned names.
import { existsSync, readdirSync, renameSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const V = String.raw`\d+\.\d+\.\d+`;

// [stable name, the file the build writes]. The order is the README's.
export const DOWNLOADS = [
  ["accshift-windows-x64-setup.exe", new RegExp(`^Accshift_${V}_x64-setup\\.exe$`)],
  ["accshift-macos-arm64.dmg", new RegExp(`^Accshift_${V}_aarch64\\.dmg$`)],
  ["accshift-linux-amd64.deb", new RegExp(`^accshift_${V}_amd64\\.deb$`)],
  ["accshift-linux-x86_64.rpm", new RegExp(`^accshift-${V}-1\\.x86_64\\.rpm$`)],
  ["accshift-linux-x86_64.AppImage", new RegExp(`^accshift_${V}_amd64\\.AppImage$`)],
  ["accshift-streamdeck.streamDeckPlugin", new RegExp(`^accshift-streamdeck_${V}\\.streamDeckPlugin$`)],
  ["accshift-cli-windows-x64.exe", new RegExp(`^accshift-cli_${V}_x64\\.exe$`)],
  ["accshift-cli-macos-arm64", new RegExp(`^accshift-cli_${V}_macos_aarch64$`)],
  ["accshift-cli-linux-x86_64", new RegExp(`^accshift-cli_${V}_linux_x86_64$`)],
];

/** The stable name of a build output (a `.sig` follows its bundle), or the name unchanged. */
export function stableName(file) {
  const sig = file.endsWith(".sig") ? ".sig" : "";
  const bundle = sig ? file.slice(0, -sig.length) : file;
  const found = DOWNLOADS.find(([, pattern]) => pattern.test(bundle));
  return found ? found[0] + sig : file;
}

function main([dir]) {
  if (!dir) throw new Error("usage: node scripts/release-assets.mjs <dir>");
  for (const file of readdirSync(dir)) {
    const name = stableName(file);
    if (name === file) continue;
    if (existsSync(join(dir, name))) throw new Error(`${name} exists twice in ${dir}`);
    renameSync(join(dir, file), join(dir, name));
    console.log(`${file} -> ${name}`);
  }
  const missing = DOWNLOADS.map(([name]) => name).filter((name) => !existsSync(join(dir, name)));
  if (missing.length > 0) throw new Error(`missing downloads: ${missing.join(", ")}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main(process.argv.slice(2));
