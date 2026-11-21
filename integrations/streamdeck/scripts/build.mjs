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

/**
 * Bundles the Node entry point into `<pluginDir>/bin/plugin.js`, the file
 * the manifest's CodePath names. The SDK's `ws` dependency is CommonJS and
 * calls `require("events")`; an ESM bundle has no `require`, so the banner
 * gives it one.
 */
export async function bundleEntry(pluginDir = PLUGIN_DIR) {
  mkdirSync(join(pluginDir, "bin"), { recursive: true });
  await build({
    entryPoints: [join(ROOT, "src", "plugin.js")],
    outfile: join(pluginDir, "bin", "plugin.js"),
    bundle: true,
    platform: "node",
    format: "esm",
    target: "node24",
    banner: {
      js: 'import { createRequire as __accshiftCreateRequire } from "node:module";\nconst require = __accshiftCreateRequire(import.meta.url);',
    },
    legalComments: "inline",
    logLevel: "warning",
  });
  writeFileSync(join(pluginDir, "bin", "package.json"), '{ "type": "module" }\n');
}

export async function buildPlugin() {
  await bundleEntry(PLUGIN_DIR);

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
