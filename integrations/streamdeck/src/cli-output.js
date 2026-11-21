// Reads the `accshift.v1` JSON envelope of the accshift CLI (docs/cli.md):
// `accshift platforms --json` and `accshift list <platform> --json`.
//
// The accounts are the platform services' raw objects, and their field names
// differ per platform (snake_case for Steam and Riot, camelCase elsewhere).
// The id read here is the one the GUI's `PlatformAccount` holds, which is the
// first thing the switch link is matched against.

export const SCHEMA = "accshift.v1";

/** An error the CLI reported, or output it could not have written. */
export class CliError extends Error {
  /**
   * @param {string} code machine code, the CLI's own or one of this plugin's
   * @param {string} message
   * @param {number | null} [exitCode]
   */
  constructor(code, message, exitCode = null) {
    super(message);
    this.name = "CliError";
    this.code = code;
    this.exitCode = exitCode;
  }
}

/** Display names of the platforms shipped with accshift; brand names are not translated. */
export const PLATFORM_NAMES = Object.freeze({
  steam: "Steam",
  riot: "Riot Games",
  "battle-net": "Battle.net",
  ubisoft: "Ubisoft Connect",
  roblox: "Roblox",
  epic: "Epic Games",
  gog: "GOG Galaxy",
  jagex: "Jagex Launcher",
  discord: "Discord",
});

/**
 * The envelope of one CLI run: its `data` on success, a CliError otherwise.
 * stdout carries a success envelope, stderr an error envelope.
 * @param {{ stdout?: string, stderr?: string, exitCode?: number | null }} run
 * @param {string} command the expected `command` field
 * @returns {any} the envelope's `data`
 */
export function readEnvelope(run, command) {
  const exitCode = run.exitCode ?? null;
  const ok = lastJsonLine(run.stdout ?? "");
  if (ok && ok.schema === SCHEMA && ok.ok === true) {
    if (ok.command !== command) {
      throw new CliError(
        "bad_output",
        `expected "${command}" output, got "${ok.command}"`,
        exitCode,
      );
    }
    return ok.data ?? {};
  }
  const failed = lastJsonLine(run.stderr ?? "") ?? ok;
  if (failed && failed.schema === SCHEMA && failed.ok === false && failed.error) {
    throw new CliError(
      String(failed.error.code ?? "error"),
      String(failed.error.message ?? ""),
      exitCode,
    );
  }
  if (failed && failed.schema && failed.schema !== SCHEMA) {
    throw new CliError("unsupported_schema", `unsupported CLI schema ${failed.schema}`, exitCode);
  }
  throw new CliError("bad_output", "the accshift CLI wrote no JSON envelope", exitCode);
}

/** The last line of `text` that parses as a JSON object, if any. */
function lastJsonLine(text) {
  const lines = String(text).split(/\r?\n/).reverse();
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed.startsWith("{")) continue;
    try {
      const value = JSON.parse(trimmed);
      if (value && typeof value === "object") return value;
    } catch {
      // Not JSON: a log line. Keep looking.
    }
  }
  return null;
}

/**
 * `accshift platforms --json`: the platform ids, with a display name.
 * @returns {{ id: string, name: string }[]}
 */
export function parsePlatforms(run) {
  const data = readEnvelope(run, "platforms");
  const list = Array.isArray(data.platforms) ? data.platforms : [];
  const seen = new Set();
  const out = [];
  for (const entry of list) {
    const id =
      typeof entry === "string" ? entry : entry && typeof entry.id === "string" ? entry.id : "";
    const key = id.trim().toLowerCase();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    const named =
      entry && typeof entry === "object" && typeof entry.name === "string" ? entry.name.trim() : "";
    out.push({ id: key, name: named || PLATFORM_NAMES[key] || key });
  }
  return out;
}

/**
 * `accshift list <platform> --json`: the accounts, the signed-in one marked,
 * sorted by name.
 * @returns {{ id: string, name: string, avatarPath: string, current: boolean }[]}
 */
export function parseAccounts(run, platformId) {
  const data = readEnvelope(run, "list");
  const platform = String(data.platform ?? platformId ?? "").toLowerCase();
  const list = Array.isArray(data.accounts) ? data.accounts : [];
  const current = typeof data.current === "string" ? data.current.trim().toLowerCase() : "";
  const seen = new Set();
  const out = [];
  for (const raw of list) {
    const account = normalizeAccount(platform, raw);
    if (!account || seen.has(account.id)) continue;
    seen.add(account.id);
    out.push({
      id: account.id,
      name: account.name,
      avatarPath: account.avatarPath,
      current: current !== "" && account.keys.includes(current),
    });
  }
  out.sort((a, b) => a.name.localeCompare(b.name, "en", { sensitivity: "base" }));
  return out;
}

/** A string field under its camelCase or snake_case name. */
function text(raw, camel) {
  const snake = camel.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);
  for (const key of [camel, snake]) {
    const value = raw[key];
    if (typeof value === "string" && value.trim() !== "") return value.trim();
    if (typeof value === "number" && Number.isFinite(value)) return String(value);
  }
  return "";
}

/** The first non-empty field. */
function first(raw, ...names) {
  for (const name of names) {
    const value = text(raw, name);
    if (value) return value;
  }
  return "";
}

/**
 * One raw account as the GUI's PlatformAccount reads it: the id the link
 * names, the name the key shows, a local avatar file if the output has one.
 * `keys` holds every value `current` may be written as.
 * @returns {{ id: string, name: string, avatarPath: string, keys: string[] } | null}
 */
export function normalizeAccount(platformId, raw) {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  let id = "";
  let name = "";
  const extra = [];
  switch (platformId) {
    case "steam":
      // to_steam_account: id steam_id, display persona_name, username account_name.
      id = first(raw, "steamId");
      name = first(raw, "personaName", "accountName");
      extra.push(text(raw, "accountName"));
      break;
    case "roblox":
      // to_roblox_account: id userId, display displayName or username.
      id = first(raw, "userId");
      name = first(raw, "displayName", "username");
      extra.push(text(raw, "username"));
      break;
    case "riot": {
      // to_riot_account: id, display the `name#tag` alias or the label.
      id = first(raw, "id");
      const alias = text(raw, "accountName");
      const tag = text(raw, "accountTagLine");
      name = alias && tag ? `${alias}#${tag}` : first(raw, "label") || alias;
      break;
    }
    case "battle-net": {
      // to_battle_net_account: id email, display the BattleTag name.
      id = first(raw, "email");
      const battleTag = text(raw, "battleTag");
      name = battleTag ? battleTag.split("#")[0].trim() : id.split("@")[0];
      break;
    }
    default:
      // Descriptor platforms (epic, gog, ubisoft, jagex, discord, user ones).
      id = first(raw, "accountId", "uuid", "id", "userId", "email", "username");
      name = first(raw, "label", "displayName", "personaName", "username");
  }
  if (!id) return null;
  if (!name) name = id;
  const keys = [id, ...extra].filter(Boolean).map((k) => k.toLowerCase());
  return { id, name, avatarPath: localAvatarPath(raw), keys };
}

/**
 * A local avatar file named by the account, if any: an absolute path, never
 * a URL. No platform writes one in `accshift.v1` today; the plugin reads it
 * when a later CLI adds it.
 */
export function localAvatarPath(raw) {
  const value = first(raw, "avatarPath", "avatarFile", "avatar");
  if (!value) return "";
  if (/^[a-z][a-z0-9+.-]*:/i.test(value) && !/^[a-zA-Z]:[\\/]/.test(value)) return "";
  const absolute =
    /^[a-zA-Z]:[\\/]/.test(value) || value.startsWith("/") || value.startsWith("\\\\");
  return absolute ? value : "";
}
