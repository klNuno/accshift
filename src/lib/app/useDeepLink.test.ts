import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSettings } from "$lib/features/settings/types";

const mocks = vi.hoisted(() => ({
  onOpenUrl: vi.fn(),
  callback: null as ((urls: string[]) => void) | null,
}));

vi.mock("@tauri-apps/plugin-deep-link", () => ({
  onOpenUrl: (...args: unknown[]) => mocks.onOpenUrl(...args),
}));

vi.mock("$lib/platforms/registry", () => ({
  getPlatformDefinition: (id: string) => (id === "steam" ? { id, name: "Steam" } : undefined),
}));

vi.mock("$lib/app/platformShell.svelte", () => ({
  isPlatformUsable: () => true,
}));

import { createDeepLinkController } from "./useDeepLink.svelte";

const settings = {
  deepLinksEnabled: true,
  enabledPlatforms: ["steam"],
} as AppSettings;

function createController(switched: boolean) {
  const showToast = vi.fn();
  const switchToAccount = vi.fn().mockResolvedValue(switched);
  const account = { id: "account-1", username: "alice", displayName: "Alice" };
  const controller = createDeepLinkController({
    t: (key) => key,
    showToast,
    getSettings: () => settings,
    getRuntimeOs: () => "windows",
    getActiveTab: () => "steam",
    isPinLocked: () => false,
    isBootReady: () => true,
    changeTab: vi.fn(),
    loadAccounts: vi.fn(),
    getAccounts: () => [account],
    isLoaderLoading: () => false,
    loadPlatformAccounts: vi.fn().mockResolvedValue([account]),
    switchToAccount,
    confirmSwitch: () => true,
  });
  return { controller, showToast, switchToAccount };
}

describe("deep-link switch result", () => {
  beforeEach(() => {
    mocks.callback = null;
    mocks.onOpenUrl.mockReset().mockImplementation(async (callback: (urls: string[]) => void) => {
      mocks.callback = callback;
      return vi.fn();
    });
  });

  it("does not announce success when the account switch returns false", async () => {
    const { controller, showToast, switchToAccount } = createController(false);
    await controller.start();

    mocks.callback?.(["accshift://switch/steam/account-1"]);
    await vi.waitFor(() => expect(switchToAccount).toHaveBeenCalledOnce());

    expect(showToast).not.toHaveBeenCalledWith("toast.deepLinkSwitched");
  });

  it("announces success only after a confirmed successful switch", async () => {
    const { controller, showToast, switchToAccount } = createController(true);
    await controller.start();

    mocks.callback?.(["accshift://switch/steam/account-1"]);
    await vi.waitFor(() => expect(switchToAccount).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(showToast).toHaveBeenCalledWith("toast.deepLinkSwitched"));
  });
});

describe("deep-link switch to another tab", () => {
  beforeEach(() => {
    mocks.callback = null;
    mocks.onOpenUrl.mockReset().mockImplementation(async (callback: (urls: string[]) => void) => {
      mocks.callback = callback;
      return vi.fn();
    });
  });

  function createCrossTab(confirmed: boolean) {
    const account = { id: "account-1", username: "alice", displayName: "Alice" };
    let activeTab = "riot";
    const events: string[] = [];
    const changeTab = vi.fn(async (tab: string) => {
      events.push(`tab:${tab}`);
      activeTab = tab;
    });
    const switchToAccount = vi.fn().mockResolvedValue(true);
    const controller = createDeepLinkController({
      t: (key) => key,
      showToast: vi.fn(),
      getSettings: () => settings,
      getRuntimeOs: () => "windows",
      getActiveTab: () => activeTab,
      isPinLocked: () => false,
      isBootReady: () => true,
      changeTab,
      loadAccounts: vi.fn(),
      // The loader only holds the active tab's accounts.
      getAccounts: () => (activeTab === "steam" ? [account] : []),
      isLoaderLoading: () => false,
      loadPlatformAccounts: vi.fn(async () => [account]),
      switchToAccount,
      confirmSwitch: async () => {
        events.push("confirm");
        return confirmed;
      },
    });
    return { controller, changeTab, switchToAccount, events };
  }

  it("asks before it changes the visible tab", async () => {
    const { controller, switchToAccount, events } = createCrossTab(true);
    await controller.start();

    mocks.callback?.(["accshift://switch/steam/account-1"]);
    await vi.waitFor(() => expect(switchToAccount).toHaveBeenCalledOnce());

    expect(events).toEqual(["confirm", "tab:steam"]);
  });

  it("loads the new tab itself when the tab change has not read it yet", async () => {
    // changeTab fires its load without awaiting it, so the list can still be
    // empty with the loading flag down.
    const account = { id: "account-1", username: "alice", displayName: "Alice" };
    let activeTab = "riot";
    let loaded: (typeof account)[] = [];
    const switchToAccount = vi.fn().mockResolvedValue(true);
    const showToast = vi.fn();
    const loadAccounts = vi.fn(async () => {
      loaded = [account];
    });
    const controller = createDeepLinkController({
      t: (key) => key,
      showToast,
      getSettings: () => settings,
      getRuntimeOs: () => "windows",
      getActiveTab: () => activeTab,
      isPinLocked: () => false,
      isBootReady: () => true,
      changeTab: vi.fn(async (tab: string) => {
        activeTab = tab;
      }),
      loadAccounts,
      getAccounts: () => loaded,
      isLoaderLoading: () => false,
      loadPlatformAccounts: vi.fn(async () => [account]),
      switchToAccount,
      confirmSwitch: async () => true,
    });
    await controller.start();

    mocks.callback?.(["accshift://switch/steam/account-1"]);
    await vi.waitFor(() => expect(switchToAccount).toHaveBeenCalledOnce());

    expect(loadAccounts).toHaveBeenCalledOnce();
    expect(showToast).not.toHaveBeenCalledWith("toast.deepLinkAccountNotFound");
  });

  it("leaves the tab alone when the switch is declined", async () => {
    const { controller, changeTab, switchToAccount, events } = createCrossTab(false);
    await controller.start();

    mocks.callback?.(["accshift://switch/steam/account-1"]);
    await vi.waitFor(() => expect(events).toContain("confirm"));
    await new Promise((done) => setTimeout(done, 0));

    expect(changeTab).not.toHaveBeenCalled();
    expect(switchToAccount).not.toHaveBeenCalled();
  });
});
