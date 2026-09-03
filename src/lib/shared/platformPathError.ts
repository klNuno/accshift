import type { MessageKey, TranslationParams } from "$lib/i18n";

type Translator = (key: MessageKey, params?: TranslationParams) => string;

/**
 * `platform_set_path` rejects a folder with a machine-readable code, a `|`,
 * then an English fallback (see `coded_path_error` in
 * `crates/accshift-core/src/platforms/steam/mod.rs`). A `PlatformError`
 * serializes to the webview as its bare message string, so the code travels
 * inside that string; matching on it is what keeps the mapping stable when
 * the English wording changes.
 */
const PATH_ERROR_MESSAGE_KEYS: Record<string, MessageKey> = {
  steam_path_never_signed_in: "settings.pathSteamNeverSignedIn",
  steam_path_not_steam: "settings.pathNotSteamFolder",
  steam_path_not_a_directory: "settings.pathNotADirectory",
};

/** Codes are lowercase snake_case, so an English message never looks like one. */
const CODE_RE = /^[a-z][a-z0-9_]*$/;

export type PlatformPathError = {
  /** The backend code, or null when the rejection carried none. */
  code: string | null;
  /** English text after the code, or the whole message when there is no code. */
  fallback: string;
};

export function parsePlatformPathError(error: unknown): PlatformPathError {
  const raw = typeof error === "string" ? error : String(error ?? "");
  const separator = raw.indexOf("|");
  if (separator === -1) return { code: null, fallback: raw };
  const code = raw.slice(0, separator);
  if (!CODE_RE.test(code)) return { code: null, fallback: raw };
  return { code, fallback: raw.slice(separator + 1) };
}

/**
 * The line to show the user for a failed path save. A known code becomes a
 * translated message; anything else keeps the caller's generic wording, so an
 * error the backend adds later never surfaces as a raw code.
 */
export function platformPathErrorMessage(
  error: unknown,
  t: Translator,
  genericMessage: string,
): string {
  const { code } = parsePlatformPathError(error);
  const key = code ? PATH_ERROR_MESSAGE_KEYS[code] : undefined;
  return key ? t(key) : genericMessage;
}
