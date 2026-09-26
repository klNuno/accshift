import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSettings } from "$lib/features/settings/types";

const mocks = vi.hoisted(() => ({ changed: [] as string[] }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/storage/clientStorage", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/storage/clientStorage")>()),
  refreshClientStorageIfChanged: async () => mocks.changed,
}));

import { CLIENT_STORE_PERSONAS, CLIENT_STORE_SETTINGS } from "$lib/storage/clientStorage";
import { createAppLifecycleController } from "./useAppLifecycle.svelte";

function createLifecycle(
  refreshPersonas: () => void,
  options: { enabledPlatforms?: string[]; calls?: string[] } = {},
) {
  const calls = options.calls ?? [];
  const enabledPlatforms = options.enabledPlatforms ?? ["steam"];
  let activeTab = "steam";
  return createAppLifecycleController({
    shell: {
      settings: { enabledPlatforms, defaultPlatformId: enabledPlatforms[0] } as AppSettings,
      get activeTab() {
        return activeTab;
      },
      runtimeOs: "windows",
      refreshSettings: vi.fn(),
      setRuntimeOs: vi.fn(),
      setActiveTab: vi.fn((tab: string) => {
        calls.push(`tab:${tab}`);
        activeTab = tab;
      }),
    },
    navigation: { currentFolderId: null, refreshCurrentItems: vi.fn() },
    loader: {
      prepareVisibleAccounts: vi.fn(),
      clearForPlatformChange: vi.fn(() => {
        calls.push("clear");
      }),
    },
    addFlow: { flow: null, cancel: vi.fn(async () => {}) },
    resetVisiblePrimeState: vi.fn(),
    loadAccounts: vi.fn(() => {
      calls.push(`load:${activeTab}`);
    }),
    queueGridPadding: vi.fn(),
    syncViewModeFromStorage: vi.fn(),
    bumpCardColorVersion: vi.fn(),
    bumpCardNoteVersion: vi.fn(),
    refreshPersonas,
    setAppVersion: vi.fn(),
    markBootReady: vi.fn(),
    replaceHistoryState: vi.fn(),
  });
}

describe("external storage refresh", () => {
  beforeEach(() => {
    mocks.changed = [];
  });

  it("reloads the personas list when another process rewrote it", async () => {
    const refreshPersonas = vi.fn();
    mocks.changed = [CLIENT_STORE_PERSONAS];

    await createLifecycle(refreshPersonas).refreshExternalStorageState();

    expect(refreshPersonas).toHaveBeenCalledOnce();
  });

  it("leaves the personas list alone for unrelated stores", async () => {
    const refreshPersonas = vi.fn();
    mocks.changed = ["something-else"];

    await createLifecycle(refreshPersonas).refreshExternalStorageState();

    expect(refreshPersonas).not.toHaveBeenCalled();
  });

  it("moves off a tab an external settings change disabled and reloads", async () => {
    // Another instance or the CLI turned Steam off while the app was in the
    // background: the grid must follow the tab, not keep Steam's accounts.
    const calls: string[] = [];
    mocks.changed = [CLIENT_STORE_SETTINGS];

    await createLifecycle(vi.fn(), {
      enabledPlatforms: ["riot"],
      calls,
    }).refreshExternalStorageState();

    expect(calls).toEqual(["clear", "tab:riot", "load:riot"]);
  });

  it("leaves the grid alone when the active tab is still enabled", async () => {
    const calls: string[] = [];
    mocks.changed = [CLIENT_STORE_SETTINGS];

    await createLifecycle(vi.fn(), {
      enabledPlatforms: ["steam", "riot"],
      calls,
    }).refreshExternalStorageState();

    expect(calls).toEqual([]);
  });
});
