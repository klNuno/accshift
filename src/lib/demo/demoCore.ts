/**
 * Demo harness: stands in for `@tauri-apps/api/core` and answers a fixed, fake
 * dataset.
 *
 * `vite.config.js` aliases `@tauri-apps/api/core` to this module when the dev
 * server is started with `VITE_DEMO=1`, so every `invoke` in the app resolves
 * here. Patching `window.__TAURI_INTERNALS__.invoke` at runtime is not an
 * option: Tauri installs it non-writable and non-configurable.
 *
 * Used to record the README capture. Two things it must guarantee:
 *
 * 1. Nothing on screen comes from the machine running the recording — every
 *    account, avatar, folder and label below is invented.
 * 2. No click can reach the real backend. A switch would stop Steam, rewrite
 *    the autologin in HKCU and relaunch the client, so `platform_switch_account`
 *    is simulated here and every unhandled platform/steam/riot/roblox command
 *    is rejected instead of forwarded.
 *
 * Boot plumbing (`log_app_event`, `finish_boot`, window controls) still goes to
 * the real backend: the window would never become visible otherwise.
 */

// Everything the real module exports except `invoke` (an explicit local export
// wins over `export *`). @tauri-apps/plugin-updater and friends import Channel
// from here, so dropping this would break their module resolution.
export * from "@tauri-core-real";

// Imported as module assets rather than dropped in public/: that folder is
// copied verbatim into every release bundle, and nothing about the demo should
// ship with the app.
import avatar1 from "./avatars/1.svg?no-inline";
import avatar2 from "./avatars/2.svg?no-inline";
import avatar3 from "./avatars/3.svg?no-inline";
import avatar4 from "./avatars/4.svg?no-inline";
import avatar5 from "./avatars/5.svg?no-inline";
import avatar6 from "./avatars/6.svg?no-inline";
import avatar7 from "./avatars/7.svg?no-inline";

const AVATARS = [avatar1, avatar2, avatar3, avatar4, avatar5, avatar6, avatar7];

interface TauriInternals {
  invoke: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>;
}

interface DemoAccount {
  steam_id: string;
  account_name: string;
  persona_name: string;
  last_login_at: number | null;
  avatar: string;
}

// Fixed timestamps (no Date.now()) so two recordings look identical.
const DAY = 86_400;
const NOW = 1_760_000_000;

const ACCOUNTS: DemoAccount[] = [
  {
    steam_id: "76561198000000001",
    account_name: "",
    persona_name: "main",
    last_login_at: NOW - 2 * 3600,
    avatar: AVATARS[0],
  },
  {
    steam_id: "76561198000000002",
    account_name: "",
    persona_name: "bro's account",
    last_login_at: NOW - 3 * DAY,
    avatar: AVATARS[1],
  },
  {
    steam_id: "76561198000000003",
    account_name: "",
    persona_name: "alt",
    last_login_at: NOW - 9 * DAY,
    avatar: AVATARS[2],
  },
  {
    steam_id: "76561198000000004",
    account_name: "",
    persona_name: "trading",
    last_login_at: NOW - 21 * DAY,
    avatar: AVATARS[3],
  },
  {
    steam_id: "76561198000000005",
    account_name: "",
    persona_name: "smurf 1",
    last_login_at: NOW - 12 * DAY,
    avatar: AVATARS[4],
  },
  {
    steam_id: "76561198000000006",
    account_name: "",
    persona_name: "smurf 2",
    last_login_at: NOW - 27 * DAY,
    avatar: AVATARS[5],
  },
  {
    steam_id: "76561198000000007",
    account_name: "",
    persona_name: "smurf 3",
    last_login_at: null,
    avatar: AVATARS[6],
  },
];

// Riot keeps captured session snapshots rather than logins, so its cards show
// a label and a state instead of a username.
const RIOT_PROFILES = [
  { id: "riot-1", label: "main", snapshot_state: "ready", last_used_at: (NOW - 3600) * 1000 },
  { id: "riot-2", label: "smurf", snapshot_state: "ready", last_used_at: (NOW - 6 * DAY) * 1000 },
  {
    id: "riot-3",
    label: "duo account",
    snapshot_state: "ready",
    last_used_at: (NOW - 19 * DAY) * 1000,
  },
];

let currentRiotProfile = RIOT_PROFILES[0].id;

const FOLDER_SMURFS = "demo-folder-smurfs";

let currentAccount = ACCOUNTS[0].steam_id;

function profileCacheEntries() {
  const entries: Record<string, { url: string; displayName: string; timestamp: number }> = {};
  for (const account of ACCOUNTS) {
    entries[account.steam_id] = {
      // Same-origin under the dev server, so `img-src 'self'` accepts it and
      // the capture never waits on avatars.steamstatic.com.
      url: new URL(account.avatar, window.location.origin).toString(),
      displayName: account.persona_name,
      timestamp: NOW * 1000,
    };
  }
  return entries;
}

function storageStores(): Record<string, unknown> {
  return {
    // Only what the capture depends on; everything else falls back to the
    // app's own defaults. English because the README is English, and streamer
    // mode off so a running OBS on the recording machine cannot blur the cards.
    "client.settings": {
      language: "en",
      enabledPlatforms: ["steam", "riot"],
      // The recording machine has reduced motion on at the OS level; "on"
      // overrides it, which is the whole point of showing the animations.
      animations: "on",
      streamerMode: "off",
      // The AFK blur would drop the ACCSHIFT curtain over the cards mid-take.
      inactivityBlurSeconds: 0,
      pinEnabled: false,
    },
    "client.folders": {
      version: 1,
      folders: [{ id: FOLDER_SMURFS, name: "Smurfs", parentId: null, platform: "steam" }],
      itemOrder: {
        "root:steam": [
          { type: "account", id: ACCOUNTS[0].steam_id },
          { type: "account", id: ACCOUNTS[1].steam_id },
          { type: "account", id: ACCOUNTS[2].steam_id },
          { type: "account", id: ACCOUNTS[3].steam_id },
          { type: "folder", id: FOLDER_SMURFS },
        ],
        [FOLDER_SMURFS]: [
          { type: "account", id: ACCOUNTS[4].steam_id },
          { type: "account", id: ACCOUNTS[5].steam_id },
          { type: "account", id: ACCOUNTS[6].steam_id },
        ],
      },
    },
    "cache.steam.profiles": profileCacheEntries(),
  };
}

function manifest(): { schemaVersion: number; stores: Record<string, string> } {
  return { schemaVersion: 1, stores: {} };
}

function storageSnapshot() {
  return { manifest: manifest(), stores: storageStores() };
}

function steamAccountsPayload() {
  return ACCOUNTS.map(({ steam_id, account_name, persona_name, last_login_at }) => ({
    steam_id,
    account_name,
    persona_name,
    last_login_at,
  }));
}

function profileInfo(steamId: string) {
  const account = ACCOUNTS.find((a) => a.steam_id === steamId);
  if (!account) return null;
  return {
    avatar_url: new URL(account.avatar, window.location.origin).toString(),
    display_name: account.persona_name,
    vac_banned: false,
    trade_ban_state: "none",
  };
}

function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

type Handler = (args: Record<string, unknown>) => unknown | Promise<unknown>;

const HANDLERS: Record<string, Handler> = {
  get_boot_payload: () => ({
    migration: "skipped",
    runtimeOs: "windows",
    storageSnapshot: storageSnapshot(),
    customThemes: [],
  }),
  load_client_storage_snapshot: () => storageSnapshot(),
  get_storage_manifest: () => manifest(),
  // Persisting is pointless for a recording, and writing would leave demo
  // folders behind in the real config.
  save_client_storage_store: () => null,
  migrate_legacy_config: () => "skipped",
  get_runtime_os: () => "windows",
  list_custom_themes: () => [],
  // Glass themes paint a captured wallpaper behind the window. That is the
  // recording machine's desktop, so the demo never gets to see it.
  get_desktop_wallpaper: () => null,
  detect_streaming_software: () => false,
  telemetry_get_state: () => ({
    mode_a_enabled: false,
    mode_b_enabled: false,
    install_id_set: false,
    forget_pending: false,
    onboarding_completed: true,
  }),
  steam_has_api_key: () => true,
  platform_get_path: () => "C:\\Program Files (x86)\\Steam",
  platform_get_accounts: (args) =>
    args.platformId === "riot"
      ? RIOT_PROFILES
      : args.platformId === "steam"
        ? steamAccountsPayload()
        : [],
  platform_get_current_account: (args) =>
    args.platformId === "riot"
      ? currentRiotProfile
      : args.platformId === "steam"
        ? currentAccount
        : "",
  platform_get_startup_snapshot: (args) => {
    if (args.platformId === "riot") {
      return { profiles: RIOT_PROFILES, currentProfile: currentRiotProfile };
    }
    if (args.platformId === "steam") {
      return { accounts: steamAccountsPayload(), currentAccount };
    }
    return { accounts: [], currentAccount: "" };
  },
  platform_switch_account: async (args) => {
    // Roughly what a real switch feels like, so the recording keeps the
    // spinner on screen long enough to read.
    await delay(900);
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
  steam_get_profile_info: (args) => profileInfo(String(args.accountId ?? args.steamId ?? "")),
  steam_get_profile_infos: (args) => {
    const ids = Array.isArray(args.accountIds) ? (args.accountIds as string[]) : [];
    const out: Record<string, unknown> = {};
    for (const id of ids) out[id] = profileInfo(id);
    return out;
  },
  steam_get_player_bans: () => [],
  steam_get_copyable_games: () => [],
  steam_get_account_games: () => [],
  cs2_bridge_get_settings: () => ({ enabled: false, url: "", apiKey: "" }),
  // Anything that would leave the window during a capture.
  open_url: () => null,
  open_logs_folder: () => null,
  steam_open_userdata: () => null,
  steam_open_api_key_page: () => null,
};

// Commands that reach a real platform. Anything here without a handler above is
// refused rather than forwarded: a stray click during a take must not touch
// Steam, the keyring or the registry.
const GUARDED_PREFIXES = ["platform_", "steam_", "riot_", "roblox_", "cs2_bridge_", "telemetry_"];

/** Real IPC, for the boot plumbing the demo does not fake. */
function realInvoke<T>(cmd: string, args?: unknown, options?: unknown): Promise<T> {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: TauriInternals })
    .__TAURI_INTERNALS__;
  if (!internals) {
    return Promise.reject(new Error("__TAURI_INTERNALS__ unavailable"));
  }
  return internals.invoke(cmd, args, options) as Promise<T>;
}

/** Drop-in replacement for `invoke` from `@tauri-apps/api/core`. */
export async function invoke<T>(cmd: string, args?: unknown, options?: unknown): Promise<T> {
  const handler = HANDLERS[cmd];
  if (handler) {
    return (await handler((args ?? {}) as Record<string, unknown>)) as T;
  }
  if (GUARDED_PREFIXES.some((prefix) => cmd.startsWith(prefix))) {
    console.warn(`[demo] blocked un-mocked command: ${cmd}`);
    throw new Error(`demo mode: ${cmd} is not available`);
  }
  return realInvoke<T>(cmd, args, options);
}

console.info("[demo] mock IPC active");
