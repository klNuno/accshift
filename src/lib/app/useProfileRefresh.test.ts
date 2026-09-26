import { describe, expect, it, vi } from "vitest";
import type { PlatformAdapter, PlatformDef } from "$lib/shared/platform";
import { createProfileRefresh } from "./useProfileRefresh";

function def(id: string, profileRefresh?: { avatars?: boolean; bans?: boolean }) {
  return { id, capabilities: { profileRefresh } } as unknown as PlatformDef;
}

function setup(adapters: Record<string, Partial<PlatformAdapter>>, activeTab = "steam") {
  const toasts: [string, unknown][] = [];
  const loadAccounts = vi.fn(async () => {});
  const ensureAdapterReady = vi.fn(async (id: string) => adapters[id] as PlatformAdapter);
  const refresh = createProfileRefresh({
    t: (key, params) => (params ? `${key}:${JSON.stringify(params)}` : key),
    showToast: (message, options) => {
      toasts.push([message, options]);
    },
    platforms: [
      def("steam", { avatars: true, bans: true }),
      def("riot"),
      def("epic", { avatars: true }),
    ],
    ensureAdapterReady,
    getActiveTab: () => activeTab,
    loadAccounts,
  });
  return { refresh, toasts, loadAccounts, ensureAdapterReady };
}

const accounts = [{ id: "a1" }, { id: "a2" }];

describe("profile refresh", () => {
  it("refreshes avatars only on platforms that declare it, reloading the active tab", async () => {
    const getProfileInfo = vi.fn(async (_accountId: string) => null);
    const { refresh, toasts, loadAccounts, ensureAdapterReady } = setup({
      steam: { loadAccounts: async () => accounts as never, getProfileInfo },
      epic: { loadAccounts: async () => [] },
    });

    await refresh.refreshAvatarsNow();

    expect(ensureAdapterReady.mock.calls.map(([id]) => id)).toEqual(["steam", "epic"]);
    expect(getProfileInfo.mock.calls.map(([id]) => id)).toEqual(["a1", "a2"]);
    expect(loadAccounts).toHaveBeenCalledWith(true, false, true, false, false);
    // Epic has no getProfileInfo: skipped before loading anything.
    expect(toasts).toEqual([['toast.avatarRefreshComplete:{"count":2}', { type: "success" }]]);
  });

  it("refreshes bans with a forced load and does not reload another tab", async () => {
    const loadWarningStates = vi.fn(async () => ({}));
    const { refresh, toasts, loadAccounts } = setup(
      { steam: { loadAccounts: async () => accounts as never, loadWarningStates } },
      "riot",
    );

    await refresh.refreshBansNow();

    expect(loadWarningStates).toHaveBeenCalledWith(accounts, {
      forceRefresh: true,
      silent: false,
      t: expect.any(Function),
    });
    expect(loadAccounts).not.toHaveBeenCalled();
    expect(toasts).toEqual([['toast.banRefreshComplete:{"count":2}', { type: "success" }]]);
  });

  it("shows the platform's empty message, and an error toast when a step throws", async () => {
    const { refresh, toasts } = setup({
      steam: {
        loadAccounts: async () => [],
        getProfileInfo: async () => null,
        getNoAccountsToastMessage: () => "no accounts",
        loadWarningStates: async () => ({}),
      },
    });
    await refresh.refreshAvatarsNow();
    expect(toasts).toEqual([["no accounts", undefined]]);

    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const failing = setup({
      steam: {
        loadAccounts: async () => {
          throw new Error("boom");
        },
        loadWarningStates: async () => ({}),
      },
    });
    await failing.refresh.refreshBansNow();
    expect(failing.toasts).toEqual([["toast.refreshFailed", { type: "error" }]]);
    error.mockRestore();
  });
});
