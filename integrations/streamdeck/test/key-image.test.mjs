import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import { avatarKeyImage, keyImage, MAX_AVATAR_BYTES, platformKeyImage } from "../src/key-image.js";
import { PLATFORM_ICON_PATHS } from "../src/platform-icons.js";
import { icnsPng } from "../scripts/icns.mjs";
import { readRustIcons } from "../scripts/sync-icons.mjs";

const svgOf = (dataUrl) => {
  assert.match(dataUrl, /^data:image\/svg\+xml;base64,/);
  return Buffer.from(dataUrl.split(",")[1], "base64").toString("utf8");
};

const files = (map) => ({
  readFile: async (p) => {
    if (!(p in map)) throw new Error("ENOENT");
    return Buffer.from(map[p]);
  },
  size: async (p) => {
    if (!(p in map)) throw new Error("ENOENT");
    return typeof map[p] === "number" ? map[p] : Buffer.byteLength(map[p]);
  },
});

describe("key images", () => {
  test("platform icons match the GUI", () => {
    const rust = readRustIcons();
    assert.equal(Object.keys(rust).length, 9);
    assert.deepEqual({ ...PLATFORM_ICON_PATHS }, rust);
  });

  test("a platform tile carries that platform's logo", () => {
    const svg = svgOf(platformKeyImage("Steam"));
    assert.match(svg, /width="144" height="144"/);
    assert.ok(svg.includes(PLATFORM_ICON_PATHS.steam));
  });

  test("a platform without a logo gets the generic tile", () => {
    const svg = svgOf(platformKeyImage("my-launcher"));
    for (const d of Object.values(PLATFORM_ICON_PATHS)) assert.ok(!svg.includes(d));
    assert.match(svg, /<path d="M12 12c2\.21/);
  });

  test("a local avatar becomes a data URL of its type", async () => {
    const io = files({ "C:\\a\\1.png": "png-bytes", "/a/2.JPG": "jpg-bytes" });
    assert.equal(
      await avatarKeyImage("C:\\a\\1.png", io),
      `data:image/png;base64,${Buffer.from("png-bytes").toString("base64")}`,
    );
    assert.match(await avatarKeyImage("/a/2.JPG", io), /^data:image\/jpeg;base64,/);
  });

  test("an unreadable, oversized or non-image avatar is skipped", async () => {
    const io = files({ "/big.png": MAX_AVATAR_BYTES + 1, "/a.txt": "text" });
    assert.equal(await avatarKeyImage("/missing.png", io), "");
    assert.equal(await avatarKeyImage("/big.png", io), "");
    assert.equal(await avatarKeyImage("/a.txt", io), "");
    assert.equal(await avatarKeyImage("", io), "");
  });

  test("the key shows the avatar, else the platform tile", async () => {
    const io = files({ "/a.png": "png" });
    assert.match(await keyImage({ platform: "steam", avatarPath: "/a.png" }, io), /^data:image\/png/);
    const tile = await keyImage({ platform: "steam", avatarPath: "/gone.png" }, io);
    assert.ok(svgOf(tile).includes(PLATFORM_ICON_PATHS.steam));
  });

  test("the plugin icon @2x is the 512 px PNG of the app icon", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const icns = readFileSync(join(here, "..", "..", "..", "crates", "accshift-egui", "icons", "icon.icns"));
    const png = icnsPng(icns, "ic09");
    assert.equal(png.readUInt32BE(0), 0x89504e47);
    assert.equal(png.readUInt32BE(16), 512);
    assert.equal(png.readUInt32BE(20), 512);
    assert.throws(() => icnsPng(icns, "zzzz"));
  });
});
