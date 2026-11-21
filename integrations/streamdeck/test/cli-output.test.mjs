import assert from "node:assert/strict";
import { describe, test } from "node:test";
import {
  CliError,
  localAvatarPath,
  parseAccounts,
  parsePlatforms,
  readEnvelope,
} from "../src/cli-output.js";

const ok = (command, data) => ({
  stdout: `${JSON.stringify({ schema: "accshift.v1", ok: true, command, data })}\n`,
  stderr: "",
  exitCode: 0,
});
const failed = (command, code, message, exitCode) => ({
  stdout: "",
  stderr: `${JSON.stringify({ schema: "accshift.v1", ok: false, command, error: { code, message } })}\n`,
  exitCode,
});
const list = (platform, accounts, current = null) =>
  ok("list", { platform, folder: null, accounts, current });

describe("CLI envelope", () => {
  test("returns the data of a success envelope after log lines", () => {
    const run = ok("platforms", { platforms: ["steam"] });
    run.stdout = `warming up\n{not json\n${run.stdout}`;
    assert.deepEqual(readEnvelope(run, "platforms"), { platforms: ["steam"] });
  });

  test("turns the CLI-disabled refusal into a CliError with exit 7", () => {
    const run = failed("platforms", "cli_disabled", "The accshift CLI is disabled", 7);
    assert.throws(
      () => readEnvelope(run, "platforms"),
      (error) => error instanceof CliError && error.code === "cli_disabled" && error.exitCode === 7,
    );
  });

  test("refuses the output of another command", () => {
    assert.throws(
      () => readEnvelope(ok("list", {}), "platforms"),
      (error) => error.code === "bad_output",
    );
  });

  test("refuses output without an envelope", () => {
    assert.throws(
      () => readEnvelope({ stdout: "steam\nriot\n", stderr: "", exitCode: 0 }, "platforms"),
      (error) => error.code === "bad_output",
    );
  });

  test("refuses a newer schema rather than guessing", () => {
    const run = {
      stdout: JSON.stringify({ schema: "accshift.v2", ok: true, command: "platforms", data: {} }),
    };
    assert.throws(
      () => readEnvelope(run, "platforms"),
      (error) => error.code === "unsupported_schema",
    );
  });
});

describe("accshift platforms --json", () => {
  test("gives each id a display name, user platforms keep their id", () => {
    const run = ok("platforms", { platforms: ["steam", "battle-net", "my-launcher", "Steam", ""] });
    assert.deepEqual(parsePlatforms(run), [
      { id: "steam", name: "Steam" },
      { id: "battle-net", name: "Battle.net" },
      { id: "my-launcher", name: "my-launcher" },
    ]);
  });

  test("an empty list is no platform", () => {
    assert.deepEqual(parsePlatforms(ok("platforms", { platforms: [] })), []);
  });
});

describe("accshift list <platform> --json", () => {
  test("Steam: steam_id is the link id, persona name the title, current by account name", () => {
    const run = list(
      "steam",
      [
        { steam_id: "76561198000000002", account_name: "bob", persona_name: "Bob the Builder", last_login_at: 2 },
        { steam_id: "76561198000000001", account_name: "alice", persona_name: "Alice", last_login_at: 1 },
        { steam_id: "76561198000000003", account_name: "carol", persona_name: "", last_login_at: 3 },
      ],
      "alice",
    );
    assert.deepEqual(parseAccounts(run, "steam"), [
      { id: "76561198000000001", name: "Alice", avatarPath: "", current: true },
      { id: "76561198000000002", name: "Bob the Builder", avatarPath: "", current: false },
      { id: "76561198000000003", name: "carol", avatarPath: "", current: false },
    ]);
  });

  test("Roblox: camelCase userId and displayName", () => {
    const run = list("roblox", [{ userId: "42", username: "builder", displayName: "", lastLoginAt: null }], "builder");
    assert.deepEqual(parseAccounts(run, "roblox"), [
      { id: "42", name: "builder", avatarPath: "", current: true },
    ]);
  });

  test("Riot: the Riot ID wins over the label", () => {
    const run = list("riot", [
      { id: "p1", label: "Main", account_name: "Player", account_tag_line: "EUW" },
      { id: "p2", label: "Smurf" },
    ]);
    assert.deepEqual(
      parseAccounts(run, "riot").map((a) => [a.id, a.name]),
      [
        ["p1", "Player#EUW"],
        ["p2", "Smurf"],
      ],
    );
  });

  test("Battle.net: the email is the id, the BattleTag name the title", () => {
    const run = list("battle-net", [
      { email: "one@example.com", battleTag: "Hero#1234" },
      { email: "two@example.com", battleTag: "" },
    ]);
    assert.deepEqual(
      parseAccounts(run, "battle-net").map((a) => [a.id, a.name]),
      [
        ["one@example.com", "Hero"],
        ["two@example.com", "two"],
      ],
    );
  });

  test("descriptor platforms: accountId and label, the id when no label", () => {
    const run = list("epic", [
      { accountId: "abc", label: "Fortnite main", lastUsedAt: 1, snapshotSaved: true },
      { accountId: "def", label: "", snapshotSaved: true },
      { label: "no id, skipped" },
      { accountId: "abc", label: "duplicate, skipped" },
      "not an object",
    ]);
    assert.deepEqual(
      parseAccounts(run, "epic").map((a) => [a.id, a.name]),
      [
        ["def", "def"],
        ["abc", "Fortnite main"],
      ],
    );
  });

  test("reads a local avatar path and ignores a URL", () => {
    const run = list("steam", [
      { steam_id: "1", persona_name: "A", avatar_path: "C:\\cache\\avatars\\1.png" },
      { steam_id: "2", persona_name: "B", avatarPath: "https://cdn.example.com/2.jpg" },
      { steam_id: "3", persona_name: "C", avatar: "/Users/x/Library/a.png" },
    ]);
    assert.deepEqual(
      parseAccounts(run, "steam").map((a) => a.avatarPath),
      ["C:\\cache\\avatars\\1.png", "", "/Users/x/Library/a.png"],
    );
  });

  test("an unknown platform error keeps the CLI code", () => {
    const run = failed("list", "platform_unavailable", "Unknown platform: nope", 2);
    assert.throws(() => parseAccounts(run, "nope"), (error) => error.code === "platform_unavailable");
  });
});

describe("avatar paths", () => {
  test("only absolute local files", () => {
    assert.equal(localAvatarPath({ avatarPath: "relative/a.png" }), "");
    assert.equal(localAvatarPath({ avatarPath: "file:///C:/a.png" }), "");
    assert.equal(localAvatarPath({ avatarPath: "\\\\server\\share\\a.png" }), "\\\\server\\share\\a.png");
    assert.equal(localAvatarPath({}), "");
  });
});
