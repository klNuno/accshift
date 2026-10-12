// Runs the real TypeScript client stores over a set of stored values and
// writes what each one saves to crates/accshift-app/tests/data/stores. The
// Rust round-trip test loads the same input, runs the same operation and
// must save the same JSON.
//
// The inputs are the client stores of every mock scenario in
// crates/accshift-egui/parity/data, the stores the TS tests build, and
// payloads with legacy keys or bad values that walk every normalization path.
//
// From the repo root:
//   build-safe node node_modules/vitest/vitest.mjs run --config crates/accshift-app/tools/vitest.dump.config.mjs
//   vp fmt crates/accshift-app/tests/data/stores
// The second step matches what the pre-commit hook does to staged JSON.
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: () => Promise.resolve(undefined) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({}), Effect: {} }));

// The stores read and write through clientStorage; an in-memory copy stands in
// for the backend and records the last value each store wrote.
const memory = vi.hoisted(() => ({
  values: new Map<string, unknown>(),
  revisions: new Map<string, number>(),
  saved: new Map<string, unknown>(),
}));

vi.mock("$lib/storage/clientStorage", async () => {
  const ids = await import("$lib/storage/storeIds");
  const bump = (id: string) => memory.revisions.set(id, (memory.revisions.get(id) ?? 0) + 1);
  return {
    ...ids,
    getClientStoreValue: (id: string) => memory.values.get(id),
    getClientStoreRevision: (id: string) => memory.revisions.get(id) ?? 0,
    onClientStoreChange: () => () => {},
    setClientStoreValue: (id: string, value: unknown) => {
      const copy = value === undefined ? undefined : JSON.parse(JSON.stringify(value));
      memory.values.set(id, copy);
      memory.saved.set(id, copy);
      bump(id);
    },
    __load: (id: string, value: unknown) => {
      memory.values.set(id, value);
      memory.saved.clear();
      bump(id);
    },
  };
});

import * as storage from "$lib/storage/clientStorage";
import { getSettings, saveSettings } from "$lib/features/settings/store";
import {
  deleteFolder,
  getItemsInFolder,
  moveItem,
  renameFolder,
  reorderItems,
  syncAccounts,
} from "$lib/features/folders/store";
import type { ItemRef } from "$lib/features/folders/types";
import { deletePersona, updatePersona } from "$lib/features/personas/store";
import { clearAccountCardNote, setAccountCardNote } from "$lib/shared/accountCardNotes";
import { setAccountCardColor, setAccountCardColors } from "$lib/shared/accountCardColors";
import { setFolderCardColor } from "$lib/shared/folderCardColors";
import { getViewMode, setViewMode } from "$lib/shared/viewMode";
import { setDefaultGame } from "$lib/platforms/steam/defaultGameStore";
import { PLATFORM_DEFS } from "$lib/platforms/registry";
import { BUILT_IN_THEMES } from "$lib/theme/themes";

const ROOT = fileURLToPath(new URL("../../../", import.meta.url));
const OUT_DIR = fileURLToPath(new URL("../tests/data/stores/", import.meta.url));
const PARITY_DIR = `${ROOT}crates/accshift-egui/parity/data/`;

type Op = { kind: string; [key: string]: unknown };
interface Case {
  name: string;
  storeId: string;
  input: unknown;
  op: Op;
}

const S = {
  settings: storage.CLIENT_STORE_SETTINGS,
  folders: storage.CLIENT_STORE_FOLDERS,
  personas: storage.CLIENT_STORE_PERSONAS,
  notes: storage.CLIENT_STORE_ACCOUNT_CARD_NOTES,
  colors: storage.CLIENT_STORE_ACCOUNT_CARD_COLORS,
  defaultGame: storage.CLIENT_STORE_ACCOUNT_DEFAULT_GAME,
  folderColors: storage.CLIENT_STORE_FOLDER_CARD_COLORS,
  viewMode: storage.CLIENT_STORE_VIEW_MODE,
} as const;

/** The operation each store runs when nothing more specific is asked: one
 * that loads the store, keeps its content and writes it back. */
const RESAVE: Record<string, Op> = {
  [S.settings]: { kind: "saveSettings" },
  [S.folders]: { kind: "reorderRoot", platform: "steam" },
  [S.personas]: { kind: "deletePersona", id: "__none__" },
  [S.notes]: { kind: "clearNote", id: "__none__" },
  [S.colors]: { kind: "setColor", id: "__none__", color: "" },
  [S.folderColors]: { kind: "setFolderColor", id: "__none__", color: "" },
  [S.defaultGame]: {
    kind: "setDefaultGame",
    id: "__roundtrip__",
    game: { appId: " 730 ", name: "Counter-Strike 2" },
  },
  [S.viewMode]: { kind: "resaveViewMode" },
};

function run(op: Op) {
  switch (op.kind) {
    case "saveSettings":
      return saveSettings(getSettings());
    case "reorderRoot":
      return reorderItems(
        null,
        op.platform as string,
        getItemsInFolder(null, op.platform as string).map((item) => ({ ...item })),
      );
    case "moveItem":
      return moveItem(
        op.item as ItemRef,
        op.from as string | null,
        op.to as string | null,
        op.platform as string,
        op.index as number | undefined,
      );
    case "deleteFolder":
      return deleteFolder(op.id as string);
    case "renameFolder":
      return renameFolder(op.id as string, op.name as string);
    case "syncAccounts":
      return syncAccounts(op.ids as string[], op.platform as string);
    case "deletePersona":
      return deletePersona(op.id as string);
    case "updatePersona":
      return updatePersona(op.id as string, op.patch as never);
    case "clearNote":
      return clearAccountCardNote(op.id as string);
    case "setNote":
      return setAccountCardNote(op.id as string, op.note as string);
    case "setColor":
      return setAccountCardColor(op.id as string, op.color as string);
    case "setColors":
      return setAccountCardColors(op.ids as string[], op.color as string);
    case "setFolderColor":
      return setFolderCardColor(op.id as string, op.color as string);
    case "setDefaultGame":
      return setDefaultGame(op.id as string, op.game as { appId: string; name: string });
    case "resaveViewMode":
      return setViewMode(getViewMode());
    default:
      throw new Error(`unknown op ${op.kind}`);
  }
}

function scenarioCases(): Case[] {
  const cases: Case[] = [];
  const seen = new Set<string>();
  for (const file of readdirSync(PARITY_DIR)
    .filter((name) => name.endsWith(".json"))
    .sort()) {
    const scenario = JSON.parse(readFileSync(`${PARITY_DIR}${file}`, "utf8"));
    const stores = scenario.commands.load_client_storage_snapshot[0].result.stores;
    for (const storeId of Object.values(S)) {
      if (!(storeId in stores)) continue;
      const key = `${storeId}:${JSON.stringify(stores[storeId])}`;
      if (seen.has(key)) continue;
      seen.add(key);
      cases.push({
        name: `${file.replace(/\.json$/, "")} ${storeId}`,
        storeId,
        input: stores[storeId],
        op: RESAVE[storeId],
      });
    }
  }
  return cases;
}

const HASH = `${"A".repeat(32)}:${"B".repeat(64)}`;

/** The folder store of the "moving an account without a drag" TS test. */
const MOVE_STORE = {
  version: 1,
  folders: [
    { id: "smurfs", name: "Smurfs", parentId: null, platform: "steam" },
    { id: "old", name: "Old", parentId: "smurfs", platform: "steam" },
    { id: "riot-folder", name: "Ranked", parentId: null, platform: "riot" },
  ],
  itemOrder: {
    "root:steam": [
      { type: "folder", id: "smurfs" },
      { type: "account", id: "at-root" },
    ],
    smurfs: [
      { type: "folder", id: "old" },
      { type: "account", id: "in-smurfs" },
    ],
    old: [{ type: "account", id: "in-old" }],
    "root:riot": [{ type: "folder", id: "riot-folder" }],
  },
};

/** The settings of the settingsPatch TS test. */
const PATCH_SETTINGS = {
  language: "en",
  themeId: "dark",
  backgroundOpacity: 100,
  uiScalePercent: 100,
  animations: "system",
  streamerMode: "auto",
  suspendGraphicsWhenMinimized: true,
  minimizeOnAccountSwitch: false,
  dataRefresh: { avatarCacheDays: 7, banCheckDays: 7 },
  enabledPlatforms: ["steam", "riot"],
  healthCheckPerPlatform: {},
  personasEnabled: true,
  defaultPlatformId: "steam",
  inactivityBlurSeconds: 60,
  deepLinksEnabled: true,
  cliEnabled: true,
  platformSettings: { steam: { runAsAdmin: false, launchOptions: "", shutdownMode: "graceful" } },
  accountDisplay: {
    showUsernames: true,
    showLastLoginPerPlatform: {},
    showCardNotesInline: false,
    expandedFolders: false,
  },
  pinEnabled: true,
  pinHash: "legacy-hash",
};

function testCases(): Case[] {
  const resave = (name: string, storeId: string, input: unknown): Case => ({
    name,
    storeId,
    input,
    op: RESAVE[storeId],
  });
  const folder = (name: string, input: unknown, op: Op): Case => ({
    name,
    storeId: S.folders,
    input,
    op,
  });
  return [
    // Fresh install: nothing stored yet.
    ...Object.values(S).map((storeId) => resave(`missing ${storeId}`, storeId, undefined)),

    // settings
    resave("settings from settingsPatch test", S.settings, PATCH_SETTINGS),
    resave("settings language only", S.settings, { language: "fr" }),
    resave("settings ui scale only", S.settings, { uiScalePercent: 110 }),
    resave("settings not an object", S.settings, ["steam"]),
    resave("settings null language", S.settings, { language: null }),
    resave("settings legacy flat keys", S.settings, {
      theme: "light",
      avatarCacheDays: "30",
      banCheckDays: null,
      showUsernames: false,
      showCardNotesInline: 1,
      expandedFolders: "",
      steamRunAsAdmin: true,
      steamLaunchOptions: "  -novid -console  ",
      accountDisplay: { showLastLogin: true, showRiotLastLogin: 0 },
    }),
    resave("settings bad values", S.settings, {
      language: "de",
      themeId: "nope",
      backgroundOpacity: 140.5,
      uiScalePercent: "abc",
      animations: "ON",
      streamerMode: "off",
      suspendGraphicsWhenMinimized: 0,
      minimizeOnAccountSwitch: "yes",
      dataRefresh: { avatarCacheDays: -3, banCheckDays: [45] },
      enabledPlatforms: ["riot", "riot", 42, "nope", "steam"],
      defaultPlatformId: "roblox",
      healthCheckPerPlatform: { roblox: null },
      inactivityBlurSeconds: 1e9,
      deepLinksEnabled: null,
      cliEnabled: false,
      personasEnabled: false,
      pinEnabled: true,
      pinHash: `  ${HASH} `,
      platformSettings: {
        steam: { runAsAdmin: "", launchOptions: "x".repeat(300), shutdownMode: "force" },
      },
      accountDisplay: {
        showUsernames: true,
        showLastLoginPerPlatform: { steam: 1, discord: false, ghost: true },
        showCardNotesInline: null,
        expandedFolders: true,
      },
    }),
    resave("settings pin without hash", S.settings, { pinEnabled: true, pinHash: "1234" }),
    resave("settings legacy pin hash", S.settings, { pinEnabled: 1, pinHash: "C".repeat(64) }),
    resave("settings hash without pin", S.settings, { pinEnabled: false, pinHash: HASH }),
    resave("settings empty platforms", S.settings, {
      enabledPlatforms: [],
      defaultPlatformId: 5,
      backgroundOpacity: "  42.5 ",
      uiScalePercent: "0x80",
      inactivityBlurSeconds: [],
      themeId: "",
      theme: "light",
    }),
    resave("settings custom theme id unknown here", S.settings, {
      themeId: "midnight",
      animations: "off",
      dataRefresh: { avatarCacheDays: 0, banCheckDays: 90.5 },
      healthCheckPerPlatform: { roblox: false },
      platformSettings: { steam: "broken" },
    }),

    // folders: the TS test stores
    folder(
      "folders ghost bucket",
      {
        version: 1,
        folders: [{ id: "live", name: "Live", parentId: null, platform: "steam" }],
        itemOrder: {
          "root:steam": [{ type: "folder", id: "live" }],
          live: [{ type: "account", id: "account-1" }],
          ghost: [{ type: "account", id: "account-2" }],
        },
      },
      RESAVE[S.folders],
    ),
    folder(
      "folders root buckets of two platforms",
      {
        version: 1,
        folders: [],
        itemOrder: {
          "root:steam": [{ type: "account", id: "account-1" }],
          "root:riot": [{ type: "account", id: "account-2" }],
        },
      },
      RESAVE[S.folders],
    ),
    folder(
      "folders bare root prefix",
      { version: 1, folders: [], itemOrder: { "root:": [{ type: "account", id: "account-1" }] } },
      RESAVE[S.folders],
    ),
    folder("folders move root account into folder", MOVE_STORE, {
      kind: "moveItem",
      item: { type: "account", id: "at-root" },
      from: null,
      to: "old",
      platform: "steam",
    }),
    folder("folders move account back to root", MOVE_STORE, {
      kind: "moveItem",
      item: { type: "account", id: "in-old" },
      from: "old",
      to: null,
      platform: "steam",
    }),
    folder("folders move at index", MOVE_STORE, {
      kind: "moveItem",
      item: { type: "account", id: "at-root" },
      from: null,
      to: "smurfs",
      platform: "steam",
      index: 0,
    }),
    folder("folders move folder into its own subtree", MOVE_STORE, {
      kind: "moveItem",
      item: { type: "folder", id: "smurfs" },
      from: null,
      to: "old",
      platform: "steam",
    }),
    folder("folders move folder to riot root", MOVE_STORE, {
      kind: "moveItem",
      item: { type: "folder", id: "old" },
      from: "smurfs",
      to: null,
      platform: "steam",
    }),
    folder("folders delete nested folder tree", MOVE_STORE, {
      kind: "deleteFolder",
      id: "smurfs",
    }),
    folder("folders delete missing folder", MOVE_STORE, { kind: "deleteFolder", id: "nope" }),
    folder("folders rename", MOVE_STORE, { kind: "renameFolder", id: "old", name: "Older" }),
    folder("folders sync accounts", MOVE_STORE, {
      kind: "syncAccounts",
      ids: ["at-root", "in-old", "new-1", "new-2"],
      platform: "steam",
    }),
    folder("folders sync unchanged", MOVE_STORE, {
      kind: "syncAccounts",
      ids: ["at-root", "in-smurfs", "in-old"],
      platform: "steam",
    }),
    folder("folders sync new platform", MOVE_STORE, {
      kind: "syncAccounts",
      ids: ["epic-1"],
      platform: "epic",
    }),
    folder(
      "folders bad values",
      {
        folders: [
          { id: " a ", name: " A ", parentId: "  ", platform: " steam " },
          { id: "b", name: "", platform: "steam" },
          { id: "c", name: "C", parentId: 5, platform: "steam" },
          "junk",
          null,
        ],
        itemOrder: {
          "root:steam": [
            { type: "folder", id: "a" },
            { type: "folder", id: "b" },
            { type: "account", id: " x " },
            { type: "account", id: "" },
            { type: "user", id: "y" },
            null,
          ],
          a: "not a list",
          c: [{ type: "folder", id: "c" }],
          "   ": [{ type: "account", id: "z" }],
        },
      },
      RESAVE[S.folders],
    ),
    folder("folders not an object", [1, 2], RESAVE[S.folders]),

    // personas
    resave("personas bad values", S.personas, [
      {
        id: "p1",
        name: "  Big brother  ",
        color: "#A1b2C3",
        image: "data:image/png;base64,AAAA",
        assignments: [
          { platformId: "steam", accountId: "s1" },
          { platformId: "steam", accountId: "s2" },
          { platformId: "ghost", accountId: "g1" },
          { platformId: "riot", accountId: "" },
          { platformId: "riot", accountId: 7 },
          { platformId: "discord", accountId: "d1" },
          [],
        ],
      },
      { id: "p1", name: "Duplicate" },
      { id: "", name: "No id" },
      { name: "No id" },
      {
        id: "p2",
        name: "   ",
        color: "#abc",
        image: "https://example.com/a.png",
        assignments: "nope",
      },
      { id: "p3", name: "n".repeat(50), color: 5, image: `data:image/png;${"a".repeat(400_001)}` },
      { id: "p4", name: "Emoji \u{1f600} name", image: null },
      "junk",
    ]),
    resave("personas not a list", S.personas, { id: "p1" }),
    {
      name: "personas update",
      storeId: S.personas,
      input: [
        { id: "p1", name: "One", color: "", image: null, assignments: [] },
        { id: "p2", name: "Two", color: "", image: null, assignments: [] },
      ],
      op: {
        kind: "updatePersona",
        id: "p2",
        patch: {
          name: "  Renamed  ",
          color: "#123456",
          assignments: [{ platformId: "roblox", accountId: "r1" }],
        },
      },
    },
    {
      name: "personas delete",
      storeId: S.personas,
      input: [
        { id: "p1", name: "One", color: "", image: null, assignments: [] },
        { id: "p2", name: "Two", color: "", image: null, assignments: [] },
      ],
      op: { kind: "deletePersona", id: "p1" },
    },

    // string maps
    resave("notes bad values", S.notes, {
      acc1: "  hello  ",
      acc2: "line1\u0000line2\u0001line3\u007f\u0085end",
      acc3: "a".repeat(300),
      acc4: "   ",
      acc5: 5,
      "  ": "blank key",
      acc6: "\u{1f600} emoji",
    }),
    {
      name: "notes set",
      storeId: S.notes,
      input: { acc1: "keep" },
      op: { kind: "setNote", id: "acc2", note: "  new\tnote  " },
    },
    {
      name: "notes set empty removes",
      storeId: S.notes,
      input: { acc1: "keep", acc2: "gone" },
      op: { kind: "setNote", id: "acc2", note: " \u0000 " },
    },
    resave("colors bad values", S.colors, {
      a: "#ff0000",
      b: "#abc",
      c: "#aabbccdd",
      d: "red",
      e: "#xyz",
      f: "#abcd",
      g: 5,
      "": "#ffffff",
    }),
    {
      name: "colors set many",
      storeId: S.colors,
      input: { keep: "#000" },
      op: { kind: "setColors", ids: ["a", "b"], color: "#3b82f6" },
    },
    {
      name: "colors set many invalid removes",
      storeId: S.colors,
      input: { a: "#000", b: "#111", c: "#222" },
      op: { kind: "setColors", ids: ["a", "b"], color: "bad" },
    },
    {
      name: "colors set many empty list",
      storeId: S.colors,
      input: { a: "#000" },
      op: { kind: "setColors", ids: [], color: "#fff" },
    },
    resave("folder colors bad values", S.folderColors, { f1: "#123456", f2: "blue" }),

    // default game
    resave("default game bad values", S.defaultGame, {
      s1: { appId: " 730 ", name: "CS2" },
      s2: { appId: "abc", name: "Bad" },
      s3: { appId: "570" },
      s4: { appId: "440", name: 5 },
      s5: [],
      s6: "730",
      "  ": { appId: "1", name: "blank key" },
    }),
    {
      name: "default game invalid set",
      storeId: S.defaultGame,
      input: { s1: { appId: "730", name: "CS2" } },
      op: { kind: "setDefaultGame", id: "s2", game: { appId: "x1", name: "Bad" } },
    },

    // view mode
    resave("view mode list", S.viewMode, "list"),
    resave("view mode other", S.viewMode, 5),
  ];
}

describe("client store round-trip dump", () => {
  it("writes what each store saves", () => {
    const load = (storage as unknown as { __load: (id: string, value: unknown) => void }).__load;
    const cases = [...scenarioCases(), ...testCases()];
    const fixtures = cases.map((entry) => {
      load(entry.storeId, entry.input === undefined ? undefined : structuredClone(entry.input));
      run(entry.op);
      const wrote = memory.saved.has(entry.storeId);
      return {
        name: entry.name,
        storeId: entry.storeId,
        input: entry.input ?? null,
        op: entry.op,
        wrote,
        expected: wrote ? memory.saved.get(entry.storeId) : null,
      };
    });
    const names = new Set(fixtures.map((fixture) => fixture.name));
    expect(names.size).toBe(fixtures.length);

    mkdirSync(OUT_DIR, { recursive: true });
    for (const name of readdirSync(OUT_DIR)) {
      if (name.endsWith(".json")) rmSync(`${OUT_DIR}${name}`);
    }
    const context = {
      platformIds: PLATFORM_DEFS.map((platform) => platform.id),
      themeIds: BUILT_IN_THEMES.map((theme) => theme.id),
      preferredLanguages: [...(globalThis.navigator?.languages ?? [])],
    };
    writeFileSync(`${OUT_DIR}context.json`, `${JSON.stringify(context, null, 2)}\n`);
    writeFileSync(`${OUT_DIR}roundtrip.json`, `${JSON.stringify(fixtures, null, 2)}\n`);
  });
});
