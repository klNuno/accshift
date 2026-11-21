import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { MAX_LINK_LENGTH, switchLink } from "../src/link.js";

// The app's parser (link.rs) splits the path on "/" and percent-decodes each
// segment; this is the same walk, to check a link reads back as written.
function parseLikeTheApp(url) {
  const rest = url.slice("accshift://".length);
  const segments = rest.split(/[?#]/)[0].split("/").filter(Boolean).map(decodeURIComponent);
  assert.equal(segments.length, 3);
  assert.equal(segments[0], "switch");
  return { platform: segments[1], account: segments[2] };
}

describe("switch link", () => {
  test("names the platform and the account id", () => {
    assert.equal(switchLink("steam", "76561198000000001"), "accshift://switch/steam/76561198000000001");
  });

  test("lowercases and trims the platform like the app", () => {
    assert.equal(switchLink(" Battle-Net ", "a@b.c"), "accshift://switch/battle-net/a%40b.c");
  });

  test("escapes every character the app would split or drop", () => {
    for (const account of ["a/b", "x#1", "q?y", "with space", "%41", "名前", "a&b=c"]) {
      const link = switchLink("riot", account);
      assert.doesNotMatch(link.slice("accshift://".length), /[#? ]/);
      assert.deepEqual(parseLikeTheApp(link), { platform: "riot", account });
    }
  });

  test("refuses a missing platform or account", () => {
    assert.throws(() => switchLink("", "1"), TypeError);
    assert.throws(() => switchLink("steam", "  "), TypeError);
    assert.throws(() => switchLink(undefined, undefined), TypeError);
  });

  test("refuses a link longer than the app parses", () => {
    assert.throws(() => switchLink("steam", "x".repeat(MAX_LINK_LENGTH)), RangeError);
  });
});
