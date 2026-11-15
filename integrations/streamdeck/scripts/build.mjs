// Builds the plugin folder dev.mtsu.accshift.sdPlugin: the Node entry
// point, the property inspector script, the Stream Deck locale files and
// the plugin icon. Everything written here is git-ignored.
//
//   node scripts/build.mjs

import { build } from "esbuild";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { icnsPng } from "./icns.mjs";
import { STREAM_DECK_LOCALES, streamDeckLocale } from "../src/i18n.js";
import { ACTION_UUID } from "../src/switch-account.js";

const here = dirname(fileURLToPath(import.meta.url));
export const ROOT = join(here, "..");
export const PLUGIN_DIR = join(ROOT, "dev.mtsu.accshift.sdPlugin");
const APP_ICONS = join(ROOT, "..", "..", "crates", "accshift-egui", "icons");

export async function buildPlugin() {
  mkdirSync(join(PLUGIN_DIR, "bin"), { recursive: true });
  await build({
    entryPoints: [join(ROOT, "src", "plugin.js")],
    outfile: join(PLUGIN_DIR, "bin", "plugin.js"),
    bundle: true,
    platform: "node",
    format: "esm",
    target: "node24",
    legalComments: "inline",
    logLevel: "warning",
  });
  writeFileSync(join(PLUGIN_DIR, "bin", "package.json"), '{ "type": "module" }\n');

  await build({
    entryPoints: [join(ROOT, "src", "inspector.js")],
    outfile: join(PLUGIN_DIR, "ui", "inspector.js"),
    bundle: true,
    platform: "browser",
    format: "iife",
    target: "chrome120",
    logLevel: "warning",
  });

  for (const [file, locale] of Object.entries(STREAM_DECK_LOCALES)) {
    const content = streamDeckLocale(locale, ACTION_UUID);
    writeFileSync(join(PLUGIN_DIR, `${file}.json`), `${JSON.stringify(content, null, 2)}\n`);
  }

  mkdirSync(join(PLUGIN_DIR, "imgs", "plugin"), { recursive: true });
  copyFileSync(join(APP_ICONS, "icon.png"), join(PLUGIN_DIR, "imgs", "plugin", "marketplace.png"));
  const icns = readFileSync(join(APP_ICONS, "icon.icns"));
  writeFileSync(join(PLUGIN_DIR, "imgs", "plugin", "marketplace@2x.png"), icnsPng(icns, "ic09"));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await buildPlugin();
  console.log(`built ${PLUGIN_DIR}`);
}
