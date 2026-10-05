import { describe, expect, it, vi } from "vitest";
import type { AppSettings } from "$lib/features/settings/types";
import type { PlatformAccount, PlatformWarningLoadOptions } from "$lib/shared/platform";
import type { BanInfo } from "./types";

vi.mock("$lib/features/notifications/store.svelte", () => ({
  addToast: vi.fn(() => "toast-id"),
  removeToast: vi.fn(),
}));

vi.mock("$lib/features/settings/store", () => ({
  peekSettings: vi.fn(() => ({ dataRefresh: { banCheckDays: 0 } })),
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

const { peekSettings } = await import("$lib/features/settings/store");
const { getPlayerBans, hasApiKey } = await import("./steamApi");
const { setClientStoreValue, CLIENT_STORE_STEAM_BAN_CHECK_STATE } =
  await import("$lib/storage/clientStorage");
const { alreadyCheckedBanIds, loadSteamWarningStates } = await import("./warnings");

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

function account(id: string): PlatformAccount {
  return { id, displayName: id, username: id };
}

function banRow(steamId: string): BanInfo {
  return {
    steam_id: steamId,
    community_banned: false,
    vac_banned: false,
    number_of_vac_bans: 0,
    days_since_last_ban: 0,
    number_of_game_bans: 0,
    economy_ban: "none",
  };
}

const quiet: PlatformWarningLoadOptions = {
  silent: true,
  t: ((key: string) => key) as PlatformWarningLoadOptions["t"],
};

describe("loadSteamWarningStates", () => {
  it("asks again for a SteamID the last response left out", async () => {
    vi.mocked(hasApiKey).mockResolvedValue(true);
    vi.mocked(getPlayerBans).mockResolvedValue([banRow("sess-kept")]);
    const accounts = [account("sess-kept"), account("sess-miss")];

    await loadSteamWarningStates(accounts, { ...quiet, forceRefresh: true });
    vi.mocked(getPlayerBans).mockClear();
    vi.mocked(getPlayerBans).mockResolvedValue([banRow("sess-miss")]);

    await loadSteamWarningStates(accounts, quiet);

    expect(vi.mocked(getPlayerBans)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(getPlayerBans).mock.calls[0]?.[0]).toEqual(["sess-miss"]);
  });

  it("does not persist a SteamID the response omitted", async () => {
    vi.mocked(peekSettings).mockReturnValue({
      dataRefresh: { avatarCacheDays: 7, banCheckDays: 7 },
    } as AppSettings);
    vi.mocked(hasApiKey).mockResolvedValue(true);
    vi.mocked(getPlayerBans).mockResolvedValue([banRow("disk-kept")]);
    vi.mocked(setClientStoreValue).mockClear();

    await loadSteamWarningStates([account("disk-kept"), account("disk-miss")], {
      ...quiet,
      forceRefresh: true,
    });

    const persisted = vi
      .mocked(setClientStoreValue)
      .mock.calls.find((call) => call[0] === CLIENT_STORE_STEAM_BAN_CHECK_STATE);
    expect(persisted?.[1]).toMatchObject({ checkedSteamIds: ["disk-kept"] });
  });
});
