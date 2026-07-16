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
