#!/usr/bin/env node
// Assembles the 1280x640 card used as the repository's social preview, the
// image GitHub shows when a link to the repo is posted anywhere.
//
// The logo and the platform glyphs are read from the app sources, so the card
// cannot drift from what the product actually looks like. Rendering it to
// docs/social-card.png is a manual step, because it needs a browser:
//
//   node scripts/build-social-card.mjs
//   open scripts/social-card.html at exactly 1280x640 and screenshot it over
//   docs/social-card.png
//
// Upload the result under Settings > General > Social preview. GitHub has no
// API for that, so it stays manual.
//
// scripts/social-card.html is generated and gitignored. Edit this file instead.

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_DIR = resolve(SCRIPT_DIR, "..");

// Order matches the Current Status table in the README.
const PLATFORMS = [
  ["steam", "Steam"],
  ["riot", "Riot Games"],
  ["battle-net", "Battle.net"],
  ["epic", "Epic Games"],
  ["ubisoft", "Ubisoft Connect"],
  ["roblox", "Roblox"],
  ["gog", "GOG Galaxy"],
  ["jagex", "Jagex Launcher"],
  ["discord", "Discord"],
];

async function loadIconPaths() {
  const src = await readFile(join(REPO_DIR, "src", "lib", "shared", "platformIcons.ts"), "utf8");
  const body = src.slice(src.indexOf("PLATFORM_ICON_PATHS"));
  const paths = {};
  for (const [, key, d] of body.matchAll(/"?([a-z-]+)"?:\s*\n?\s*"((?:[^"\\]|\\.)*)"/g)) {
    paths[key] = d;
  }
  return paths;
}

// Tight bounds of the two white paths inside the logo's 2000x2000 viewBox,
// measured with getBoundingClientRect. The source file wraps the mark in a
// rounded rect with a lot of dead margin, so drawing it as-is wastes most of
// the space it occupies.
const MARK_BOX = { x: 431, y: 682, w: 1139, h: 637 };
const MARK_PAD = 12;

async function loadLogo() {
  const svg = await readFile(join(REPO_DIR, "public", "logo.svg"), "utf8");
  const viewBox = [
    MARK_BOX.x - MARK_PAD,
    MARK_BOX.y - MARK_PAD,
    MARK_BOX.w + MARK_PAD * 2,
    MARK_BOX.h + MARK_PAD * 2,
  ].join(" ");

  return (
    svg
      .replace(/<\?xml[^>]*\?>\s*/, "")
      // The rounded plate is the same colour as the card, so it contributes
      // nothing but padding. Dropping it lets the mark run to the crop.
      .replace(/<rect\b[^>]*\/>/, "")
      .replace(/\s(width|height)="\d+"/g, "")
      .replace(/viewBox="[^"]*"/, `viewBox="${viewBox}"`)
  );
}

const icon = (d, label) =>
  `<svg viewBox="0 0 24 24" role="img" aria-label="${label}"><path d="${d}"/></svg>`;

async function main() {
  const paths = await loadIconPaths();
  const missing = PLATFORMS.filter(([key]) => !paths[key]).map(([key]) => key);
  if (missing.length) throw new Error(`no icon path for: ${missing.join(", ")}`);

  const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>accshift social card</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  html, body { width: 1280px; height: 640px; overflow: hidden; }
  body {
    background: #171717;
    color: #ffffff;
    font-family: Inter, "Segoe UI", system-ui, sans-serif;
    display: flex;
    flex-direction: column;
    /* Three blocks, evenly spread, so no single gap collects all the slack. */
    justify-content: space-between;
    padding: 62px 76px 56px;
  }

  .brand { display: flex; align-items: center; gap: 28px; }
  .brand svg { height: 96px; width: auto; display: block; }
  .brand span {
    font-size: 58px; font-weight: 640; letter-spacing: -0.04em;
  }

  h1 {
    font-size: 76px; line-height: 1.0; letter-spacing: -0.045em;
    font-weight: 700;
  }

  .sub {
    margin-top: 26px;
    font-size: 26px; line-height: 1.4; letter-spacing: -0.012em;
    color: #8c8c8c; max-width: 900px;
  }

  /* Label and glyphs share one baseline row, so the strip uses the full width
     instead of stacking into the empty right half. */
  .platforms {
    display: flex; align-items: center; gap: 44px;
    padding-top: 34px; border-top: 1px solid #2c2c2c;
  }
  .platforms .label {
    flex: none;
    font-size: 14px; font-weight: 600; letter-spacing: 0.18em;
    text-transform: uppercase; color: #6e6e6e;
  }
  .platforms .row {
    display: flex; align-items: center; justify-content: space-between;
    flex: 1;
  }
  .platforms svg { width: 44px; height: 44px; fill: #e8e8e8; display: block; }
</style>
</head>
<body>
  <div class="brand">
    ${(await loadLogo()).trim()}
    <span>accshift</span>
  </div>

  <div class="copy">
    <h1>Switch game accounts in one click</h1>
    <p class="sub">No passwords stored. Free and open source for Windows, macOS and Linux.</p>
  </div>

  <div class="platforms">
    <span class="label">Works with</span>
    <div class="row">
      ${PLATFORMS.map(([key, label]) => icon(paths[key], label)).join("\n      ")}
    </div>
  </div>
</body>
</html>
`;

  await mkdir(SCRIPT_DIR, { recursive: true });
  await writeFile(join(SCRIPT_DIR, "social-card.html"), html, "utf8");
  console.log(`social-card.html written with ${PLATFORMS.length} platform glyphs`);
}

await main();
