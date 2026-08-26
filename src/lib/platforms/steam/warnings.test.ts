import { describe, expect, it } from "vitest";
import { vi } from "vitest";

vi.mock("$lib/features/notifications/store.svelte", () => ({
  addToast: vi.fn(() => "toast-id"),
  removeToast: vi.fn(),
}));

vi.mock("$lib/features/settings/store", () => ({
  peekSettings: () => ({ dataRefresh: { banCheckDays: 0 } }),
}));

vi.mock("./steamApi", () => ({
  getPlayerBans: vi.fn(async () => []),
  hasApiKey: vi.fn(async () => false),
}));

vi.mock("$lib/storage/clientStorage", () => ({
  CLIENT_STORE_STEAM_BAN_CHECK_STATE: "cache.steam.ban-check-state",
  CLIENT_STORE_STEAM_BAN_INFO_CACHE: "cache.steam.ban-info-cache",
  getClientStoreValue: vi.fn(() => null),
  setClientStoreValue: vi.fn(),
}));

const { alreadyCheckedBanIds } = await import("./warnings");

const session = new Set(["session-only"]);
const cached = new Set(["cached-only"]);

function plan(overrides: Partial<Parameters<typeof alreadyCheckedBanIds>[0]> = {}) {
  return alreadyCheckedBanIds({
    forceRefresh: false,
    delayDays: 7,
    withinDelayWindow: true,
    sessionCheckedIds: session,
    cachedCheckedIds: cached,
    ...overrides,
  });
}

describe("alreadyCheckedBanIds", () => {
  it("checks everything again on a forced refresh", () => {
    expect(plan({ forceRefresh: true }).size).toBe(0);
    // Even with a delay window still open.
    expect(plan({ forceRefresh: true, delayDays: 0 }).size).toBe(0);
  });

  it("skips only what this session fetched when no delay is configured", () => {
    expect(plan({ delayDays: 0 })).toBe(session);
  });

  it("skips the persisted set inside the delay window", () => {
    expect(plan()).toBe(cached);
  });

  it("checks everything again once the delay window has lapsed", () => {
    expect(plan({ withinDelayWindow: false }).size).toBe(0);
  });
});
