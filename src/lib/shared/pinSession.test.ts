import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mocks.invoke(...args),
}));

import {
  PIN_FAILURE_DELAY_MS,
  isPinLockedError,
  lockPinSession,
  onPinLockedError,
  pinFailureWait,
  reportPinLockedError,
  unlockPinSession,
} from "./pinSession";

// The cross-check vector of pin.test.ts: PIN 4321 under a fixed salt.
const PBKDF2_HASH =
  "000102030405060708090a0b0c0d0e0f:e19d9507e40b77fbb7503faedce7cb4ebf8c6820a8b746d9dfa9fcab899ec65d";
const LOCKED = "pin_locked: Accshift is locked. Enter the PIN to switch accounts.";

beforeEach(() => {
  mocks.invoke.mockReset();
  vi.spyOn(console, "error").mockImplementation(() => {});
});

describe("pin_locked errors", () => {
  it("recognizes the backend refusal as a string or an Error", () => {
    expect(isPinLockedError(LOCKED)).toBe(true);
    expect(isPinLockedError(new Error(LOCKED))).toBe(true);
    expect(isPinLockedError("Steam is not installed")).toBe(false);
    expect(isPinLockedError(undefined)).toBe(false);
  });

  it("tells the lock screen, and only about that refusal", () => {
    const listener = vi.fn();
    const stop = onPinLockedError(listener);

    expect(reportPinLockedError("Steam is not installed")).toBe(false);
    expect(listener).not.toHaveBeenCalled();
    expect(reportPinLockedError(LOCKED)).toBe(true);
    expect(listener).toHaveBeenCalledOnce();

    stop();
    reportPinLockedError(LOCKED);
    expect(listener).toHaveBeenCalledOnce();
  });
});

describe("unlockPinSession", () => {
  it("unlocks through the backend", async () => {
    mocks.invoke.mockResolvedValue({ status: "unlocked", legacy: false, retryAfterMs: 0 });

    const check = await unlockPinSession("4321", PBKDF2_HASH);

    expect(mocks.invoke).toHaveBeenCalledWith("pin_unlock", { code: "4321" });
    expect(check).toEqual({ status: "match", rehashed: null });
  });

  it("rehashes a legacy PIN the backend accepted", async () => {
    mocks.invoke.mockResolvedValue({ status: "unlocked", legacy: true, retryAfterMs: 0 });

    const check = await unlockPinSession("1234", "");

    expect(check.status).toBe("match");
    expect(check.status === "match" && check.rehashed).toMatch(/^[a-f0-9]{32}:[a-f0-9]{64}$/);
  });

  it("takes the backend's verdict on a wrong code, whatever the local hash says", async () => {
    mocks.invoke.mockResolvedValue({ status: "invalid", legacy: false, retryAfterMs: 8000 });

    // 4321 matches the local hash: the backend still wins.
    const check = await unlockPinSession("4321", PBKDF2_HASH);

    expect(check).toEqual({ status: "invalid", retryAfterMs: 8000 });
  });

  it("passes a rate-limit refusal through", async () => {
    mocks.invoke.mockResolvedValue({ status: "retry_later", legacy: false, retryAfterMs: 4000 });

    expect(await unlockPinSession("4321", PBKDF2_HASH)).toEqual({
      status: "retry_later",
      retryAfterMs: 4000,
    });
  });

  it("checks the local hash when the backend has no PIN on disk yet", async () => {
    mocks.invoke.mockResolvedValue({ status: "not_configured", legacy: false, retryAfterMs: 0 });

    expect(await unlockPinSession("4321", PBKDF2_HASH)).toEqual({
      status: "match",
      rehashed: null,
    });
    expect(await unlockPinSession("1111", PBKDF2_HASH)).toEqual({
      status: "invalid",
      retryAfterMs: 0,
    });
  });

  it("checks the local hash when the backend call fails", async () => {
    mocks.invoke.mockRejectedValue(new Error("ipc down"));

    expect((await unlockPinSession("1111", PBKDF2_HASH)).status).toBe("invalid");
    expect((await unlockPinSession("4321", PBKDF2_HASH)).status).toBe("match");
  });

  it("lets the screen go when neither side holds a PIN", async () => {
    mocks.invoke.mockResolvedValue({ status: "not_configured", legacy: false, retryAfterMs: 0 });

    expect(await unlockPinSession("0000", "")).toEqual({ status: "match", rehashed: null });
  });
});

describe("lockPinSession", () => {
  it("locks the backend and swallows a failure", async () => {
    mocks.invoke.mockRejectedValue(new Error("ipc down"));

    lockPinSession();
    await Promise.resolve();

    expect(mocks.invoke).toHaveBeenCalledWith("pin_lock");
  });
});

describe("pinFailureWait", () => {
  it("keeps the local pause for a plain wrong code", () => {
    expect(pinFailureWait({ status: "invalid", retryAfterMs: 1000 })).toEqual({
      waitMs: PIN_FAILURE_DELAY_MS,
      tooManyAttempts: false,
    });
  });

  it("follows the backend once its wait is the longer one", () => {
    expect(pinFailureWait({ status: "invalid", retryAfterMs: 8000 })).toEqual({
      waitMs: 8000,
      tooManyAttempts: true,
    });
    expect(pinFailureWait({ status: "retry_later", retryAfterMs: 500 })).toEqual({
      waitMs: PIN_FAILURE_DELAY_MS,
      tooManyAttempts: true,
    });
  });
});
