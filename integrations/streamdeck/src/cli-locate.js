// Where the accshift CLI lives once the desktop app is installed.
//
// Windows: the NSIS installer (crates/accshift-egui/tauri.conf.json,
// `installMode: currentUser`) puts `accshift.exe` (the `externalBin`
// `binaries/accshift`) next to `accshift-gui.exe` in
// `%LOCALAPPDATA%\Accshift`, adds that folder to the user PATH
// (bundle/path.nsh) and the GUI registers its own path as the `accshift://`
// handler under `HKCU\Software\Classes\accshift\shell\open\command`
// (lifecycle/deep_link/registration.rs). The handler is read first, so a
// custom install folder is found too.
//
// macOS: the `.app` bundles the CLI as `Contents/MacOS/accshift`
// (docs/cli.md), in /Applications or ~/Applications; a symlink the user made
// on the PATH comes last.
//
// ACCSHIFT_CLI overrides all of them, for development.

import path from "node:path";

export const HANDLER_KEY = "HKCU\\Software\\Classes\\accshift\\shell\\open\\command";

/**
 * The executable a `reg query` of the handler key names, from its
 * `"C:\...\accshift-gui.exe" "%1"` value. The value name column is localized
 * ("(Default)", "(par défaut)"), so the line is found by its type.
 * @param {string} output
 * @returns {string}
 */
export function handlerExecutable(output) {
  for (const line of String(output ?? "").split(/\r?\n/)) {
    const match = line.match(/\sREG_(?:EXPAND_)?SZ\s+(.*)$/);
    if (!match) continue;
    const value = match[1].trim();
    const quoted = value.match(/^"([^"]+)"/);
    if (quoted) return quoted[1];
    const bare = value.split(/\s+"?%1/)[0].trim();
    if (bare) return bare;
  }
  return "";
}

/**
 * The places to look for the CLI, best first, without touching the disk.
 * @param {{ platform: string, env: Record<string, string | undefined>, home?: string, handlerOutput?: string }} input
 * @returns {string[]}
 */
export function cliCandidates({ platform, env, home = "", handlerOutput = "" }) {
  const out = [];
  const add = (p) => {
    if (p && !out.includes(p)) out.push(p);
  };
  if (env.ACCSHIFT_CLI) add(env.ACCSHIFT_CLI);
  if (platform === "win32") {
    const win = path.win32;
    const gui = handlerExecutable(handlerOutput);
    if (gui) add(win.join(win.dirname(gui), "accshift.exe"));
    if (env.LOCALAPPDATA) add(win.join(env.LOCALAPPDATA, "Accshift", "accshift.exe"));
    for (const dir of pathEntries(env.Path ?? env.PATH ?? "", ";")) {
      add(win.join(dir, "accshift.exe"));
    }
  } else if (platform === "darwin") {
    const mac = path.posix;
    add("/Applications/Accshift.app/Contents/MacOS/accshift");
    if (home) add(mac.join(home, "Applications", "Accshift.app", "Contents", "MacOS", "accshift"));
    for (const dir of ["/usr/local/bin", "/opt/homebrew/bin"]) add(mac.join(dir, "accshift"));
    for (const dir of pathEntries(env.PATH ?? "", ":")) add(mac.join(dir, "accshift"));
  } else {
    for (const dir of pathEntries(env.PATH ?? "", ":")) add(path.posix.join(dir, "accshift"));
  }
  return out;
}

function pathEntries(value, separator) {
  return String(value)
    .split(separator)
    .map((entry) => entry.trim().replace(/^"(.*)"$/, "$1"))
    .filter(Boolean);
}

/**
 * The first candidate that is a file, or "" when accshift is not installed.
 * @param {{ platform: string, env: Record<string, string | undefined>, home?: string,
 *   readHandler?: () => Promise<string>, isFile: (p: string) => Promise<boolean> }} io
 * @returns {Promise<{ path: string, tried: string[] }>}
 */
export async function locateCli({ platform, env, home, readHandler, isFile }) {
  let handlerOutput = "";
  if (platform === "win32" && readHandler) {
    try {
      handlerOutput = await readHandler();
    } catch {
      handlerOutput = "";
    }
  }
  const tried = cliCandidates({ platform, env, home, handlerOutput });
  for (const candidate of tried) {
    if (await isFile(candidate)) return { path: candidate, tried };
  }
  return { path: "", tried };
}
