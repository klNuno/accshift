import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const manifest = JSON.parse(
  readFileSync(new URL("../dev.mtsu.accshift.sdPlugin/manifest.json", import.meta.url), "utf8"),
);

// The macOS app registers no accshift:// scheme and does not receive URL
// Apple Events, so a key press there cannot switch: Windows only until it does.
test("the manifest declares Windows only", () => {
  assert.deepEqual(manifest.OS, [{ Platform: "windows", MinimumVersion: "10" }]);
});
