/**
 * The datasets the mock can boot on. Picked at build time (`VITE_MOCK=<id>`)
 * and, for everything but `demo`, switchable at runtime with `__mock.use()`.
 *
 * `demo` is a public artifact: it produces `docs/demo-*.webp`. Treat it as
 * frozen — anything you need for a one-off check belongs in `dev` or in a new
 * scenario, never in there.
 */
import {
  DAY,
  HOUR,
  NOW,
  type MockAccount,
  type MockSpec,
  foldersStore,
  generateAccounts,
} from "./fixtures";

// Imported as module assets rather than dropped in public/: that folder is
// copied verbatim into every release bundle, and nothing here should ever ship.
// `?no-inline` because under 4 kB vite would inline them as `data:` URIs, which
// `isSafeHttpUrl` rejects (http/https only) — the app would fall back to
// initials and the avatars would silently vanish.
import avatar1 from "./avatars/1.svg?no-inline";
import avatar2 from "./avatars/2.svg?no-inline";
import avatar3 from "./avatars/3.svg?no-inline";
import avatar4 from "./avatars/4.svg?no-inline";
import avatar5 from "./avatars/5.svg?no-inline";
import avatar6 from "./avatars/6.svg?no-inline";
import avatar7 from "./avatars/7.svg?no-inline";

export const AVATARS = [avatar1, avatar2, avatar3, avatar4, avatar5, avatar6, avatar7];

const STEAM_PATH_WINDOWS = "C:\\Program Files (x86)\\Steam";

/** Settings every scenario needs, whatever else it overrides. */
const BASE_SETTINGS = {
  language: "en",
  // Machines used for capture and for agent runs have OS reduced-motion on;
  // "on" overrides it, and animations are half of what there is to look at.
  animations: "on",
  // Streamer mode would blur the cards if OBS happened to be running.
  streamerMode: "off",
  // The AFK blur drops the ACCSHIFT curtain over the UI mid-session.
  inactivityBlurSeconds: 0,
  pinEnabled: false,
};

function baseSpec(): MockSpec {
  return {
    label: "",
    runtimeOs: "windows",
    steamAccounts: [],
    currentSteamAccount: "",
    riotProfiles: [],
    currentRiotProfile: "",
    stores: { "client.settings": { ...BASE_SETTINGS, enabledPlatforms: ["steam"] } },
    steamPath: STEAM_PATH_WINDOWS,
    hasSteamApiKey: true,
    switchDelayMs: 900,
  };
}

// ------------------------------------------------------------------- demo

const DEMO_ACCOUNTS: MockAccount[] = [
  {
    steam_id: "76561198000000001",
    account_name: "",
    persona_name: "main",
    last_login_at: NOW - 2 * HOUR,
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

const DEMO_FOLDER = "demo-folder-smurfs";

function demoScenario(): MockSpec {
  return {
    ...baseSpec(),
    label: "README recording dataset (frozen)",
    steamAccounts: DEMO_ACCOUNTS,
    currentSteamAccount: DEMO_ACCOUNTS[0].steam_id,
    // Riot keeps captured session snapshots rather than logins, so its cards
    // show a label and a state instead of a username.
    riotProfiles: [
      { id: "riot-1", label: "main", snapshot_state: "ready", last_used_at: (NOW - HOUR) * 1000 },
      {
        id: "riot-2",
        label: "smurf",
        snapshot_state: "ready",
        last_used_at: (NOW - 6 * DAY) * 1000,
      },
      {
        id: "riot-3",
        label: "duo account",
        snapshot_state: "ready",
        last_used_at: (NOW - 19 * DAY) * 1000,
      },
    ],
    currentRiotProfile: "riot-1",
    stores: {
      "client.settings": { ...BASE_SETTINGS, enabledPlatforms: ["steam", "riot"] },
      "client.folders": foldersStore(
        [{ id: DEMO_FOLDER, name: "Smurfs", parentId: null, platform: "steam" }],
        {
          "root:steam": [
            { type: "account", id: DEMO_ACCOUNTS[0].steam_id },
            { type: "account", id: DEMO_ACCOUNTS[1].steam_id },
            { type: "account", id: DEMO_ACCOUNTS[2].steam_id },
            { type: "account", id: DEMO_ACCOUNTS[3].steam_id },
            { type: "folder", id: DEMO_FOLDER },
          ],
          [DEMO_FOLDER]: [
            { type: "account", id: DEMO_ACCOUNTS[4].steam_id },
            { type: "account", id: DEMO_ACCOUNTS[5].steam_id },
            { type: "account", id: DEMO_ACCOUNTS[6].steam_id },
          ],
        },
      ),
    },
  };
}

// -------------------------------------------------------------------- dev

// One account per rendering path the real config will not produce on demand:
// a username shown next to the persona, a missing avatar (initials fallback),
// a VAC ban badge, a name long enough to overflow, a never-logged-in card.
const DEV_ACCOUNTS: MockAccount[] = [
  {
    steam_id: "76561198100000001",
    account_name: "mainacct",
    persona_name: "main",
    last_login_at: NOW - HOUR,
    avatar: AVATARS[0],
  },
  {
    steam_id: "76561198100000002",
    account_name: "no_avatar_here",
    persona_name: "no avatar",
    last_login_at: NOW - 2 * DAY,
    avatar: null,
  },
  {
    steam_id: "76561198100000003",
    account_name: "banned_one",
    persona_name: "vac banned",
    last_login_at: NOW - 5 * DAY,
    avatar: AVATARS[2],
    vacBanned: true,
    gameBans: 1,
  },
  {
    steam_id: "76561198100000004",
    account_name: "trade_locked",
    persona_name: "trade banned",
    last_login_at: NOW - 8 * DAY,
    avatar: AVATARS[3],
    tradeBanState: "banned",
  },
  {
    steam_id: "76561198100000005",
    account_name: "overflow_test_account_name",
    persona_name: "a persona name long enough to overflow every card layout",
    last_login_at: NOW - 14 * DAY,
    avatar: AVATARS[4],
  },
  {
    steam_id: "76561198100000006",
    account_name: "never_used",
    persona_name: "never logged in",
    last_login_at: null,
    avatar: AVATARS[5],
  },
  {
    steam_id: "76561198100000007",
    account_name: "nested_one",
    persona_name: "nested account",
    last_login_at: NOW - 30 * DAY,
    avatar: AVATARS[6],
  },
  {
    steam_id: "76561198100000008",
    account_name: "nested_two",
    persona_name: "deeply nested",
    last_login_at: NOW - 44 * DAY,
    avatar: AVATARS[1],
  },
];

const DEV_FOLDER = "dev-folder-outer";
const DEV_SUBFOLDER = "dev-folder-inner";

function devScenario(): MockSpec {
  return {
    ...baseSpec(),
    label: "agent playground — edge cases, safe to change",
    steamAccounts: DEV_ACCOUNTS,
    currentSteamAccount: DEV_ACCOUNTS[0].steam_id,
    // One profile per snapshot_state, which is the whole state machine of the
    // Riot cards and cannot be reproduced from a real config on demand.
    riotProfiles: [
      { id: "riot-1", label: "ready", snapshot_state: "ready", last_used_at: (NOW - HOUR) * 1000 },
      { id: "riot-2", label: "capturing", snapshot_state: "capturing", last_used_at: null },
      { id: "riot-3", label: "awaiting", snapshot_state: "awaiting_capture", last_used_at: null },
      { id: "riot-4", label: "setup pending", snapshot_state: "setup_pending", last_used_at: null },
    ],
    currentRiotProfile: "riot-1",
    stores: {
      "client.settings": {
        ...BASE_SETTINGS,
        enabledPlatforms: ["steam", "riot"],
        accountDisplay: { showUsernames: true, showCardNotesInline: true },
      },
      "client.folders": foldersStore(
        [
          { id: DEV_FOLDER, name: "Outer folder", parentId: null, platform: "steam" },
          { id: DEV_SUBFOLDER, name: "Inner folder", parentId: DEV_FOLDER, platform: "steam" },
        ],
        {
          "root:steam": [
            { type: "account", id: DEV_ACCOUNTS[0].steam_id },
            { type: "account", id: DEV_ACCOUNTS[1].steam_id },
            { type: "account", id: DEV_ACCOUNTS[2].steam_id },
            { type: "account", id: DEV_ACCOUNTS[3].steam_id },
            { type: "account", id: DEV_ACCOUNTS[4].steam_id },
            { type: "account", id: DEV_ACCOUNTS[5].steam_id },
            { type: "folder", id: DEV_FOLDER },
          ],
          [DEV_FOLDER]: [
            { type: "account", id: DEV_ACCOUNTS[6].steam_id },
            { type: "folder", id: DEV_SUBFOLDER },
          ],
          [DEV_SUBFOLDER]: [{ type: "account", id: DEV_ACCOUNTS[7].steam_id }],
        },
      ),
      "client.account-card-notes": {
        [DEV_ACCOUNTS[0].steam_id]: "short note",
        [DEV_ACCOUNTS[4].steam_id]:
          "a note that runs long enough to test wrapping and truncation on an inline card note",
      },
      "client.account-card-colors": {
        [DEV_ACCOUNTS[0].steam_id]: "#3b82f6",
        [DEV_ACCOUNTS[2].steam_id]: "#f43f5e",
        [DEV_ACCOUNTS[4].steam_id]: "#10b981",
      },
    },
  };
}

// ------------------------------------------------------- empty / huge / slow

function emptyScenario(): MockSpec {
  return {
    ...baseSpec(),
    label: "no account anywhere — empty states and first-run UI",
    hasSteamApiKey: false,
    steamPath: "",
  };
}

function hugeScenario(): MockSpec {
  const accounts = generateAccounts(200, AVATARS);
  return {
    ...baseSpec(),
    label: "200 accounts — scroll, grid padding, virtualization",
    steamAccounts: accounts,
    currentSteamAccount: accounts[0].steam_id,
  };
}

function slowScenario(): MockSpec {
  return {
    ...devScenario(),
    label: "dev dataset with a 4s switch — spinners and pending states",
    switchDelayMs: 4000,
  };
}

export const SCENARIOS: Record<string, () => MockSpec> = {
  demo: demoScenario,
  dev: devScenario,
  empty: emptyScenario,
  huge: hugeScenario,
  slow: slowScenario,
};

export const DEFAULT_SCENARIO = "dev";
