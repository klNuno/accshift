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

import { CLIENT_STORE_PERSONAS } from "$lib/storage/clientStorage";
import { createAppLifecycleController } from "./useAppLifecycle.svelte";

function createLifecycle(refreshPersonas: () => void) {
  return createAppLifecycleController({
    shell: {
      settings: { enabledPlatforms: ["steam"] } as AppSettings,
      activeTab: "steam",
      runtimeOs: "windows",
      refreshSettings: vi.fn(),
      setRuntimeOs: vi.fn(),
      setActiveTab: vi.fn(),
    },
    navigation: { currentFolderId: null, refreshCurrentItems: vi.fn() },
    loader: { prepareVisibleAccounts: vi.fn() },
    loadAccounts: vi.fn(),
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
});
