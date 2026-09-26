import { beforeEach, describe, expect, it, vi } from "vitest";

const settings = vi.hoisted(() => ({ current: { pinEnabled: false, pinHash: "" } }));

vi.mock("$lib/features/settings/store", () => ({
  peekSettings: () => settings.current,
}));

import { createHandlers, type MockSpec } from "./fixtures";

// The cross-check vector of pin.test.ts: PIN 4321 under a fixed salt.
const PIN_HASH =
  "000102030405060708090a0b0c0d0e0f:e19d9507e40b77fbb7503faedce7cb4ebf8c6820a8b746d9dfa9fcab899ec65d";

function spec(stored: Record<string, unknown>): MockSpec {
  return {
    label: "test",
    runtimeOs: "windows",
    steamAccounts: [],
    currentSteamAccount: "",
    riotProfiles: [],
    currentRiotProfile: "",
    stores: { "client.settings": stored },
    steamPath: "",
    hasSteamApiKey: false,
    switchDelayMs: 0,
  };
}

const SWITCH = { platformId: "steam", accountId: "a" };

describe("mock PIN session", () => {
  beforeEach(() => {
    settings.current = { pinEnabled: true, pinHash: PIN_HASH };
  });

  it("starts locked when the scenario carries a PIN, and a good code unlocks it", async () => {
    const handlers = createHandlers(spec({ pinEnabled: true, pinHash: PIN_HASH }));

    await expect(handlers.platform_switch_account(SWITCH)).rejects.toMatch(/^pin_locked:/);
    expect(await handlers.pin_unlock({ code: "1111" })).toMatchObject({ status: "invalid" });
    expect(await handlers.pin_unlock({ code: "4321" })).toMatchObject({ status: "unlocked" });
    await expect(handlers.platform_switch_account(SWITCH)).resolves.toBeNull();

    handlers.pin_lock({});
    await expect(handlers.platform_switch_account(SWITCH)).rejects.toMatch(/^pin_locked:/);
  });

  it("stays unlocked for a PIN set during the session, until the next lock", async () => {
    const handlers = createHandlers(spec({}));

    await expect(handlers.platform_switch_account(SWITCH)).resolves.toBeNull();
    handlers.pin_lock({});
    await expect(handlers.platform_switch_account(SWITCH)).rejects.toMatch(/^pin_locked:/);
  });

  it("gates nothing without a PIN", async () => {
    settings.current = { pinEnabled: false, pinHash: "" };
    const handlers = createHandlers(spec({}));

    handlers.pin_lock({});
    await expect(handlers.platform_switch_account(SWITCH)).resolves.toBeNull();
    expect(await handlers.pin_unlock({ code: "0000" })).toMatchObject({
      status: "not_configured",
    });
  });
});
