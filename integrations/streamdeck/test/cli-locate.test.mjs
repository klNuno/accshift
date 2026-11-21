import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { cliCandidates, handlerExecutable, locateCli } from "../src/cli-locate.js";

const REG_EN = `
HKEY_CURRENT_USER\\Software\\Classes\\accshift\\shell\\open\\command
    (Default)    REG_SZ    "D:\\Games\\Accshift\\accshift-gui.exe" "%1"

`;
const REG_FR = REG_EN.replace("(Default)", "(par défaut)");

describe("Windows handler key", () => {
  test("reads the GUI path, whatever the language of the value name", () => {
    assert.equal(handlerExecutable(REG_EN), "D:\\Games\\Accshift\\accshift-gui.exe");
    assert.equal(handlerExecutable(REG_FR), "D:\\Games\\Accshift\\accshift-gui.exe");
  });

  test("no handler, no path", () => {
    assert.equal(handlerExecutable(""), "");
    assert.equal(handlerExecutable("ERROR: The system was unable to find the specified registry key"), "");
  });
});

describe("CLI candidates", () => {
  test("Windows: the handler's folder, then the installer folder, then PATH", () => {
    const env = {
      LOCALAPPDATA: "C:\\Users\\u\\AppData\\Local",
      Path: "C:\\Windows;\"C:\\Tools\";C:\\Users\\u\\AppData\\Local\\Accshift",
    };
    assert.deepEqual(cliCandidates({ platform: "win32", env, handlerOutput: REG_EN }), [
      "D:\\Games\\Accshift\\accshift.exe",
      "C:\\Users\\u\\AppData\\Local\\Accshift\\accshift.exe",
      "C:\\Windows\\accshift.exe",
      "C:\\Tools\\accshift.exe",
    ]);
  });

  test("macOS: inside the .app bundles, then the PATH", () => {
    assert.deepEqual(cliCandidates({ platform: "darwin", env: { PATH: "/usr/bin" }, home: "/Users/u" }), [
      "/Applications/Accshift.app/Contents/MacOS/accshift",
      "/Users/u/Applications/Accshift.app/Contents/MacOS/accshift",
      "/usr/local/bin/accshift",
      "/opt/homebrew/bin/accshift",
      "/usr/bin/accshift",
    ]);
  });

  test("ACCSHIFT_CLI comes first", () => {
    const [first] = cliCandidates({ platform: "win32", env: { ACCSHIFT_CLI: "X:\\dev\\accshift.exe" } });
    assert.equal(first, "X:\\dev\\accshift.exe");
  });
});

describe("locateCli", () => {
  test("returns the first candidate that is a file", async () => {
    const found = await locateCli({
      platform: "win32",
      env: { LOCALAPPDATA: "C:\\L" },
      readHandler: async () => REG_EN,
      isFile: async (p) => p === "C:\\L\\Accshift\\accshift.exe",
    });
    assert.equal(found.path, "C:\\L\\Accshift\\accshift.exe");
  });

  test("not installed: no path, every place tried listed", async () => {
    const found = await locateCli({
      platform: "win32",
      env: { LOCALAPPDATA: "C:\\L" },
      readHandler: async () => {
        throw new Error("reg.exe missing");
      },
      isFile: async () => false,
    });
    assert.equal(found.path, "");
    assert.deepEqual(found.tried, ["C:\\L\\Accshift\\accshift.exe"]);
  });
});
