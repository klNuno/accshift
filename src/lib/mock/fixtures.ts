/**
 * Fixture builders shared by every mock scenario.
 *
 * A scenario is data (`MockSpec`); this file turns that data into the handler
 * map the fake `invoke` dispatches on. Keeping the two apart is what lets the
 * frozen README dataset and the throwaway dev datasets answer the exact same
 * command surface.
 */
import type { RiotProfile } from "$lib/platforms/riot/types";
import type { BanInfo, ProfileInfo, SteamAccount } from "$lib/platforms/steam/types";

// Fixed clock. No Date.now() anywhere in the mock, so two runs — and two
// recordings — produce identical "3 days ago" labels.
export const NOW = 1_760_000_000;
export const DAY = 86_400;
export const HOUR = 3_600;

export type Handler = (args: Record<string, unknown>) => unknown | Promise<unknown>;

export interface MockAccount extends SteamAccount {
  /** null exercises the app's colored-initials fallback instead of an image. */
  avatar: string | null;
  vacBanned?: boolean;
  tradeBanState?: string;
  gameBans?: number;
}

export interface MockSpec {
  /** Shown in the boot console line. */
  label: string;
  /** Drives both `get_runtime_os` and the boot payload: "windows" | "macos" | "linux". */
  runtimeOs: string;
  steamAccounts: MockAccount[];
  currentSteamAccount: string;
  riotProfiles: RiotProfile[];
  currentRiotProfile: string;
  /** `client.*` / `cache.*` stores, merged over the generated avatar cache. */
  stores: Record<string, unknown>;
  steamPath: string;
  hasSteamApiKey: boolean;
  /** What `platform_switch_account` pretends to cost, in ms. */
  switchDelayMs: number;
  /** Extra or overriding handlers, applied last. */
  handlers?: Record<string, Handler>;
}

function detectedPlatforms(spec: MockSpec): string[] {
  const settings = spec.stores["client.settings"] as { enabledPlatforms?: unknown } | undefined;
  const enabled = settings?.enabledPlatforms;
  if (!Array.isArray(enabled)) return [];
  return enabled.filter((id): id is string => typeof id === "string");
}

export function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Same-origin under the dev server, so `img-src 'self'` accepts it. */
function assetUrl(path: string): string {
  return new URL(path, window.location.origin).toString();
}

function steamAccountsPayload(accounts: MockAccount[]): SteamAccount[] {
  return accounts.map(({ steam_id, account_name, persona_name, last_login_at }) => ({
    steam_id,
    account_name,
    persona_name,
    last_login_at,
  }));
}

function profileInfo(account: MockAccount | undefined): ProfileInfo | null {
  if (!account) return null;
  return {
    avatar_url: account.avatar ? assetUrl(account.avatar) : null,
    display_name: account.persona_name,
    vac_banned: account.vacBanned ?? false,
    trade_ban_state: account.tradeBanState ?? "none",
  };
}

function banInfo(account: MockAccount): BanInfo {
  return {
    steam_id: account.steam_id,
    community_banned: false,
    vac_banned: account.vacBanned ?? false,
    number_of_vac_bans: account.vacBanned ? 1 : 0,
    days_since_last_ban: account.vacBanned ? 412 : 0,
    number_of_game_bans: account.gameBans ?? 0,
    economy_ban: account.tradeBanState ?? "none",
  };
}

function profileCacheEntries(accounts: MockAccount[]) {
  const entries: Record<string, { url: string; displayName: string; timestamp: number }> = {};
  for (const account of accounts) {
    if (!account.avatar) continue;
    entries[account.steam_id] = {
      url: assetUrl(account.avatar),
      displayName: account.persona_name,
      timestamp: NOW * 1000,
    };
  }
  return entries;
}

/**
 * Every client store, with a non-null empty value.
 *
 * Not cosmetic: `clientStorage.ts` treats any store missing from the snapshot
 * as un-migrated and fills it from the legacy `localStorage` keys — which, on a
 * dev machine, hold the real personas, notes, card colors, view mode and cached
 * Steam avatars. A partial snapshot therefore puts real data on screen. Null
 * counts as missing there (`memoryStores.get(id) == null`), so these have to be
 * real empty values.
 */
const EMPTY_STORES: Record<string, unknown> = {
  "client.settings": {},
  "client.folders": { version: 1, folders: [], itemOrder: {} },
  "client.personas": [],
  "client.account-card-notes": {},
  "client.account-card-colors": {},
  "client.account-default-game": {},
  "client.folder-card-colors": {},
  "client.view-mode": "grid",
  "cache.steam.profiles": {},
  "cache.roblox.profiles": {},
  "cache.steam.ban-check-state": {},
  "cache.steam.ban-info-cache": {},
};

/** A `client.folders` store, in the shape the folder feature persists. */
export function foldersStore(
  folders: { id: string; name: string; parentId: string | null; platform: string }[],
  itemOrder: Record<string, { type: "account" | "folder"; id: string }[]>,
) {
  return { version: 1, folders, itemOrder };
}

/** Filler accounts for the stress scenarios (long lists, scroll, grid padding). */
export function generateAccounts(count: number, avatars: string[]): MockAccount[] {
  return Array.from({ length: count }, (_, index) => ({
    steam_id: `7656119900000${String(index).padStart(4, "0")}`,
    account_name: `filler_${index + 1}`,
    persona_name: `account ${index + 1}`,
    last_login_at: index === 0 ? NOW - HOUR : NOW - (index % 45) * DAY,
    avatar: avatars.length ? avatars[index % avatars.length] : null,
  }));
}

/**
 * Turns a scenario into the command map the fake `invoke` dispatches on.
 *
 * The mutable bits (which account is current) live in this closure: a reload
 * rebuilds them from the spec, which is the only reset the mock needs.
 */
export function createHandlers(spec: MockSpec): Record<string, Handler> {
  let currentAccount = spec.currentSteamAccount;
  let currentRiotProfile = spec.currentRiotProfile;

  const findAccount = (id: string) => spec.steamAccounts.find((a) => a.steam_id === id);

  const stores = (): Record<string, unknown> => ({
    ...EMPTY_STORES,
    "cache.steam.profiles": profileCacheEntries(spec.steamAccounts),
    ...spec.stores,
  });

  const manifest = () => ({ schemaVersion: 1, stores: {} });
  const snapshot = () => ({ manifest: manifest(), stores: stores() });

  const handlers: Record<string, Handler> = {
    get_boot_payload: () => ({
      migration: "skipped",
      runtimeOs: spec.runtimeOs,
      storageSnapshot: snapshot(),
      customThemes: [],
      // A mock session never reads the real descriptor folder: a screenshot
      // must not depend on what the recording machine has lying in it.
      userPlatforms: { dir: "", loaded: [], skipped: [], rejected: [] },
    }),
    load_client_storage_snapshot: () => snapshot(),
    get_storage_manifest: () => manifest(),
    // Deliberately a no-op: a mock session must never leave folders, colors or
    // settings behind in the real config. `__mock.seed()` is the way to start
    // from a given store.
    save_client_storage_store: () => null,
    migrate_legacy_config: () => "skipped",
    get_runtime_os: () => spec.runtimeOs,
    list_custom_themes: () => [],
    // Glass themes paint a captured wallpaper behind the window. That is the
    // real desktop of whoever is running this, so the mock never yields one.
    get_desktop_wallpaper: () => null,
    detect_streaming_software: () => false,
    // Never probes the real disk: a scenario declares which launchers its
    // machine has through the platforms it enables, and detection answers
    // exactly that. Otherwise a capture would depend on what is installed on
    // the recording machine.
    platform_detect_installed: () => detectedPlatforms(spec),
    telemetry_get_state: () => ({
      mode_a_enabled: false,
      mode_b_enabled: false,
      install_id_set: false,
      forget_pending: false,
      onboarding_completed: true,
    }),
    steam_has_api_key: () => spec.hasSteamApiKey,
    platform_get_path: () => spec.steamPath,
    platform_get_accounts: (args) =>
      args.platformId === "riot"
        ? spec.riotProfiles
        : args.platformId === "steam"
          ? steamAccountsPayload(spec.steamAccounts)
          : [],
    platform_get_current_account: (args) =>
      args.platformId === "riot"
        ? currentRiotProfile
        : args.platformId === "steam"
          ? currentAccount
          : "",
    platform_get_startup_snapshot: (args) => {
      if (args.platformId === "riot") {
        return { profiles: spec.riotProfiles, currentProfile: currentRiotProfile };
      }
      if (args.platformId === "steam") {
        return { accounts: steamAccountsPayload(spec.steamAccounts), currentAccount };
      }
      return { accounts: [], currentAccount: "" };
    },
    platform_switch_account: async (args) => {
      // Roughly what a real switch feels like, so the spinner stays on screen
      // long enough to read (and to record).
      await delay(spec.switchDelayMs);
      if (typeof args.accountId === "string") {
        if (args.platformId === "riot") {
          currentRiotProfile = args.accountId;
        } else {
          currentAccount = args.accountId;
        }
      }
      return null;
    },
    platform_set_account_label: () => null,
    steam_get_profile_info: (args) =>
      profileInfo(findAccount(String(args.accountId ?? args.steamId ?? ""))),
    steam_get_profile_infos: (args) => {
      const ids = Array.isArray(args.accountIds) ? (args.accountIds as string[]) : [];
      const out: Record<string, ProfileInfo | null> = {};
      for (const id of ids) out[id] = profileInfo(findAccount(id));
      return out;
    },
    steam_get_player_bans: (args) => {
      const ids = Array.isArray(args.steamIds) ? (args.steamIds as string[]) : [];
      return ids
        .map((id) => findAccount(id))
        .filter((account): account is MockAccount => Boolean(account))
        .map(banInfo);
    },
    steam_get_copyable_games: () => [],
    steam_get_account_games: () => [],
    cs2_bridge_get_settings: () => ({ enabled: false, url: "", apiKey: "" }),
    // Anything that would pull focus out of the window.
    open_url: () => null,
    open_logs_folder: () => null,
    steam_open_userdata: () => null,
    steam_open_api_key_page: () => null,
  };

  return { ...handlers, ...spec.handlers };
}
