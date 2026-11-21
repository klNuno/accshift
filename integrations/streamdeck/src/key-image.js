// The key image: the account's avatar when the CLI names a local file,
// else the platform logo on a dark tile.

import path from "node:path";
import { PLATFORM_ICON_PATHS } from "./platform-icons.js";

/** Brand colours of the tiles; any other platform gets the accshift purple. */
const TILE_COLORS = {
  steam: "#1b2838",
  riot: "#d13639",
  "battle-net": "#148eff",
  ubisoft: "#0070ff",
  roblox: "#393b3d",
  epic: "#2a2a2a",
  gog: "#86328a",
  jagex: "#2b2b2b",
  discord: "#5865f2",
};
const DEFAULT_TILE = "#a855f7";

/** A generic person, for platforms without a logo (user descriptors). */
const FALLBACK_PATH =
  "M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z";

const IMAGE_TYPES = {
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
  ".bmp": "image/bmp",
  ".svg": "image/svg+xml",
};

/** Avatars above this size are not sent to Stream Deck. */
export const MAX_AVATAR_BYTES = 2 * 1024 * 1024;

/**
 * The 144x144 SVG tile of a platform, as a data URL Stream Deck accepts.
 * @param {string} platformId
 */
export function platformKeyImage(platformId) {
  const id = String(platformId ?? "").toLowerCase();
  const d = PLATFORM_ICON_PATHS[id] ?? FALLBACK_PATH;
  const fill = TILE_COLORS[id] ?? DEFAULT_TILE;
  const svg =
    '<svg xmlns="http://www.w3.org/2000/svg" width="144" height="144" viewBox="0 0 144 144">' +
    `<rect width="144" height="144" rx="24" fill="${fill}"/>` +
    `<g transform="translate(36 30) scale(3)" fill="#ffffff"><path d="${d}"/></g>` +
    "</svg>";
  return `data:image/svg+xml;base64,${Buffer.from(svg, "utf8").toString("base64")}`;
}

/**
 * The avatar file as a data URL, or "" when it is not an image this plugin
 * sends or cannot be read.
 * @param {string} file absolute path from the CLI output
 * @param {{ readFile: (p: string) => Promise<Buffer>, size: (p: string) => Promise<number> }} io
 */
export async function avatarKeyImage(file, io) {
  if (!file) return "";
  const type = IMAGE_TYPES[path.extname(file).toLowerCase()];
  if (!type) return "";
  try {
    if ((await io.size(file)) > MAX_AVATAR_BYTES) return "";
    const bytes = await io.readFile(file);
    return `data:${type};base64,${Buffer.from(bytes).toString("base64")}`;
  } catch {
    return "";
  }
}

/** The image a configured key shows: the avatar first, the platform tile otherwise. */
export async function keyImage(settings, io) {
  const avatar = await avatarKeyImage(settings.avatarPath ?? "", io);
  return avatar || platformKeyImage(settings.platform ?? "");
}
