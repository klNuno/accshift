import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { Accshift, openCommand } from "../src/accshift.js";
import {
  accountOptions,
  platformOptions,
  settingsForAccount,
  settingsForPlatform,
} from "../src/inspector-model.js";
import { NOTICE_MS, SwitchAccount } from "../src/switch-account.js";

const CLI = "C:\\L\\Accshift\\accshift.exe";
const envelope = (command, data) =>
  JSON.stringify({ schema: "accshift.v1", ok: true, command, data });

/** A machine with accshift installed (or not), every command recorded. */
function fakeIo({ installed = true, platform = "win32", replies = {}, openExit = 0 } = {}) {
  const calls = [];
  return {
    calls,
    platform,
    env: { LOCALAPPDATA: "C:\\L" },
    home: "/Users/u",
    readHandler: async () => "",
    isFile: async (p) => installed && (p === CLI || p.endsWith("/Contents/MacOS/accshift")),
    readFile: async () => {
      throw new Error("ENOENT");
    },
    size: async () => {
      throw new Error("ENOENT");
    },
    run: async (file, args) => {
      calls.push([file, ...args]);
      if (file === CLI || file.endsWith("accshift")) {
        const reply = replies[args[0]];
        if (reply) return reply;
      }
      return { stdout: "", stderr: openExit ? "no application can open the URL" : "", exitCode: openExit };
    },
  };
}

function fakeLog() {
  const lines = [];
  return {
    lines,
    info: (m) => lines.push(["info", m]),
    warn: (m) => lines.push(["warn", m]),
    error: (m) => lines.push(["error", m]),
  };
}

function fakeKey() {
  const events = [];
  return {
    events,
    setTitle: async (t) => events.push(["title", t]),
    setImage: async (i) => events.push(["image", i]),
    showAlert: async () => events.push(["alert"]),
    showOk: async () => events.push(["ok"]),
  };
}

function controller(io, log, language = "en") {
  const timers = [];
  const action = new SwitchAccount(new Accshift(io, log), {
    language: () => language,
    setTimeout: (fn, ms) => timers.push([fn, ms]),
  });
  return { action, timers };
}

const SETTINGS = { platform: "steam", accountId: "76561198000000001", accountName: "Alice" };

describe("key press", () => {
  test("opens the switch link through the OS handler and shows OK", async () => {
    const io = fakeIo();
    const log = fakeLog();
    const key = fakeKey();
    await controller(io, log).action.keyDown(key, SETTINGS);
    assert.deepEqual(io.calls, [
      ["rundll32.exe", "url.dll,FileProtocolHandler", "accshift://switch/steam/76561198000000001"],
    ]);
    assert.deepEqual(key.events, [["ok"]]);
    assert.deepEqual(log.lines, [["info", "switch link opened for steam"]]);
  });

  test("never runs `accshift switch`: the GUI keeps the PIN and the CLI rules", async () => {
    const io = fakeIo();
    await controller(io, fakeLog()).action.keyDown(fakeKey(), SETTINGS);
    assert.ok(io.calls.every((call) => call[0] !== CLI));
  });

  test("not installed: alert, a short title for a while, and a log line", async () => {
    const io = fakeIo({ installed: false });
    const log = fakeLog();
    const key = fakeKey();
    const { action, timers } = controller(io, log, "fr");
    await action.keyDown(key, SETTINGS);
    assert.deepEqual(io.calls, []);
    assert.deepEqual(key.events, [["alert"], ["title", "Pas\nd'accshift"]]);
    assert.equal(log.lines.length, 1);
    assert.equal(log.lines[0][0], "warn");
    assert.match(log.lines[0][1], /^accshift is not installed: no accshift CLI at C:\\L\\Accshift\\accshift\.exe/);
    assert.equal(timers.length, 1);
    assert.equal(timers[0][1], NOTICE_MS);
    timers[0][0]();
    assert.deepEqual(key.events.at(-1), ["title", "Alice"]);
  });

  test("a key without an account alerts and opens nothing", async () => {
    const io = fakeIo();
    const key = fakeKey();
    await controller(io, fakeLog()).action.keyDown(key, { platform: "steam" });
    assert.deepEqual(io.calls, []);
    assert.deepEqual(key.events, [["alert"]]);
  });

  test("macOS: `open` failing alerts and logs without the account", async () => {
    const io = fakeIo({ platform: "darwin", openExit: 1 });
    const log = fakeLog();
    const key = fakeKey();
    await controller(io, log).action.keyDown(key, SETTINGS);
    assert.deepEqual(io.calls, [["open", "accshift://switch/steam/76561198000000001"]]);
    assert.deepEqual(key.events, [["alert"]]);
    assert.equal(log.lines[0][0], "error");
    assert.doesNotMatch(log.lines[0][1], /76561198000000001/);
  });

  test("open commands per OS", () => {
    assert.deepEqual(openCommand("darwin", "accshift://x"), { file: "open", args: ["accshift://x"] });
    assert.deepEqual(openCommand("linux", "accshift://x"), { file: "xdg-open", args: ["accshift://x"] });
  });
});

describe("key look", () => {
  test("the title is the account name, the image the platform tile", async () => {
    const key = fakeKey();
    await controller(fakeIo(), fakeLog()).action.render(key, SETTINGS);
    assert.deepEqual(key.events[0], ["title", "Alice"]);
    assert.match(key.events[1][1], /^data:image\/svg\+xml;base64,/);
  });

  test("an unconfigured key keeps the manifest image", async () => {
    const key = fakeKey();
    await controller(fakeIo(), fakeLog()).action.render(key, {});
    assert.deepEqual(key.events, [
      ["title", ""],
      ["image", undefined],
    ]);
  });
});

describe("property inspector requests", () => {
  test("getPlatforms runs `accshift platforms --json`", async () => {
    const io = fakeIo({
      replies: { platforms: { stdout: envelope("platforms", { platforms: ["steam", "gog"] }), stderr: "", exitCode: 0 } },
    });
    const sent = [];
    await controller(io, fakeLog()).action.message({ event: "getPlatforms" }, async (p) => sent.push(p));
    assert.deepEqual(io.calls, [[CLI, "platforms", "--json"]]);
    assert.deepEqual(sent, [
      { event: "platforms", items: [{ id: "steam", name: "Steam" }, { id: "gog", name: "GOG Galaxy" }] },
    ]);
  });

  test("getAccounts runs `accshift list <platform> --json`", async () => {
    const data = { platform: "gog", folder: null, accounts: [{ accountId: "9", label: "Main" }], current: "9" };
    const io = fakeIo({ replies: { list: { stdout: envelope("list", data), stderr: "", exitCode: 0 } } });
    const sent = [];
    await controller(io, fakeLog()).action.message({ event: "getAccounts", platform: "gog" }, async (p) => sent.push(p));
    assert.deepEqual(io.calls, [[CLI, "list", "gog", "--json"]]);
    assert.deepEqual(sent, [
      { event: "accounts", platform: "gog", items: [{ id: "9", name: "Main", avatarPath: "", current: true }] },
    ]);
  });

  test("CLI turned off in accshift: the inspector says where to turn it on", async () => {
    const refusal = JSON.stringify({
      schema: "accshift.v1",
      ok: false,
      command: "platforms",
      error: { code: "cli_disabled", message: "disabled" },
    });
    const io = fakeIo({ replies: { platforms: { stdout: "", stderr: refusal, exitCode: 7 } } });
    const sent = [];
    await controller(io, fakeLog(), "fr").action.message({ event: "getPlatforms" }, async (p) => sent.push(p));
    assert.equal(sent[0].event, "error");
    assert.match(sent[0].message, /Autoriser le CLI accshift/);
  });

  test("not installed: the inspector says so", async () => {
    const sent = [];
    await controller(fakeIo({ installed: false }), fakeLog()).action.message(
      { event: "getPlatforms" },
      async (p) => sent.push(p),
    );
    assert.equal(sent[0].event, "error");
    assert.match(sent[0].message, /accshift is not installed/);
  });
});

describe("inspector lists", () => {
  test("a saved platform the CLI no longer lists stays selected", () => {
    const options = platformOptions("en", [{ id: "steam", name: "Steam" }], "gone");
    assert.deepEqual(options.at(-1), { value: "gone", label: "gone", selected: true });
    assert.equal(options[0].label, "Choose a platform");
  });

  test("the signed-in account is marked, the saved one selected", () => {
    const items = [
      { id: "1", name: "Alice", current: true },
      { id: "2", name: "Bob", current: false },
    ];
    const options = accountOptions("en", items, { accountId: "2" });
    assert.deepEqual(
      options.map((o) => [o.label, o.selected]),
      [
        ["Choose an account", false],
        ["Alice (signed in)", false],
        ["Bob", true],
      ],
    );
    assert.equal(accountOptions("en", [], {})[0].label, "No accounts on this platform.");
  });

  test("a platform choice clears the account, an account choice saves its name and avatar", () => {
    assert.deepEqual(settingsForPlatform("gog"), {
      platform: "gog",
      accountId: "",
      accountName: "",
      avatarPath: "",
    });
    assert.deepEqual(settingsForAccount("gog", { id: "9", name: "Main", avatarPath: "/a.png" }), {
      platform: "gog",
      accountId: "9",
      accountName: "Main",
      avatarPath: "/a.png",
    });
  });
});
