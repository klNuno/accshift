// What the plugin asks of the installed accshift: find its CLI, run it for
// the property inspector, and open a switch link for a key press. Every
// side effect goes through `io`, so tests run with no accshift and no
// Stream Deck.

import { execFile } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import os from "node:os";
import { HANDLER_KEY, locateCli } from "./cli-locate.js";
import { CliError, parseAccounts, parsePlatforms } from "./cli-output.js";
import { switchLink } from "./link.js";

/** How long one CLI call may take before it is abandoned. */
export const CLI_TIMEOUT_MS = 15_000;

/**
 * The command that hands `url` to the OS, which starts the registered
 * `accshift://` handler: the running GUI receives it as a second launch.
 * @returns {{ file: string, args: string[] }}
 */
export function openCommand(platform, url) {
  if (platform === "win32") {
    return { file: "rundll32.exe", args: ["url.dll,FileProtocolHandler", url] };
  }
  if (platform === "darwin") return { file: "open", args: [url] };
  return { file: "xdg-open", args: [url] };
}

/** The real side effects. */
export function nodeIo() {
  const run = (file, args) =>
    new Promise((resolve) => {
      execFile(
        file,
        args,
        { timeout: CLI_TIMEOUT_MS, windowsHide: true, maxBuffer: 8 * 1024 * 1024 },
        (error, stdout, stderr) => {
          const exitCode =
            error && typeof error.code === "number" ? error.code : error ? null : 0;
          resolve({
            stdout: String(stdout ?? ""),
            stderr: String(stderr ?? ""),
            exitCode,
            spawnError: error && typeof error.code !== "number" ? error : null,
          });
        },
      );
    });
  return {
    platform: process.platform,
    env: process.env,
    home: os.homedir(),
    run,
    readHandler: async () => {
      const out = await run("reg.exe", ["query", HANDLER_KEY, "/ve"]);
      return out.exitCode === 0 ? out.stdout : "";
    },
    isFile: async (p) => {
      try {
        return (await stat(p)).isFile();
      } catch {
        return false;
      }
    },
    readFile: (p) => readFile(p),
    size: async (p) => (await stat(p)).size,
  };
}

export class Accshift {
  /**
   * @param {ReturnType<typeof nodeIo>} io
   * @param {{ info: (m: string) => void, warn: (m: string) => void, error: (m: string) => void }} log
   */
  constructor(io, log) {
    this.io = io;
    this.log = log;
  }

  /** The CLI path, or a `not_installed` CliError after a log line naming every place tried. */
  async cli() {
    const found = await locateCli(this.io);
    if (found.path) return found.path;
    this.log.warn(
      `accshift is not installed: no accshift CLI at ${found.tried.join(", ") || "any known place"}`,
    );
    throw new CliError("not_installed", "accshift is not installed");
  }

  /** One CLI command with `--json`, its envelope checked. */
  async runJson(args) {
    const cli = await this.cli();
    const out = await this.io.run(cli, [...args, "--json"]);
    if (out.spawnError) {
      this.log.error(`accshift CLI could not start (${cli}): ${out.spawnError.message}`);
      throw new CliError("not_installed", out.spawnError.message);
    }
    return out;
  }

  async platforms() {
    return parsePlatforms(await this.runJson(["platforms"]));
  }

  async accounts(platformId) {
    return parseAccounts(await this.runJson(["list", platformId]), platformId);
  }

  /**
   * Opens the switch link of a configured key. Throws `not_configured`,
   * `not_installed` or `open_failed`.
   */
  async switchTo(settings) {
    if (!settings?.platform || !settings?.accountId) {
      throw new CliError("not_configured", "the key has no account yet");
    }
    await this.cli();
    const url = switchLink(settings.platform, settings.accountId);
    const { file, args } = openCommand(this.io.platform, url);
    const out = await this.io.run(file, args);
    if (out.spawnError || (out.exitCode !== 0 && this.io.platform !== "win32")) {
      // rundll32 reports nothing useful; `open` and `xdg-open` exit non-zero
      // when no app answers the scheme.
      const reason = out.spawnError?.message || out.stderr.trim() || `exit ${out.exitCode}`;
      this.log.error(`could not open the accshift switch link for ${settings.platform}: ${reason}`);
      throw new CliError("open_failed", reason);
    }
    this.log.info(`switch link opened for ${settings.platform}`);
    return url;
  }
}

/** The PI's text for a failure, as an i18n key. */
export function errorKey(error) {
  if (error instanceof CliError) {
    if (error.code === "not_installed") return "error.notInstalled";
    if (error.code === "cli_disabled" || error.exitCode === 7) return "error.cliDisabled";
  }
  return "error.failed";
}
