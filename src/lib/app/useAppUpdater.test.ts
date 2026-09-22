import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  relaunch: vi.fn(),
  trackUpdate: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: (...args: unknown[]) => mocks.check(...args),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: (...args: unknown[]) => mocks.relaunch(...args),
}));

vi.mock("$lib/app/telemetryClient", () => ({
  trackUpdate: (...args: unknown[]) => mocks.trackUpdate(...args),
}));

import { classifyUpdateCheckError, createAppUpdater } from "./useAppUpdater.svelte";

/** The exact Display strings of tauri-plugin-updater 2.10.1 src/error.rs. */
const TARGET_NOT_FOUND =
  "the platform `linux-x86_64` was not found in the response `platforms` object";
const TARGETS_NOT_FOUND =
  'None of the fallback platforms `["darwin-aarch64-app", "darwin-aarch64"]` ' +
  "were found in the response `platforms` object";

function updater() {
  return createAppUpdater({
    t: (key) => key,
    addToast: vi.fn(),
  });
}

describe("update check error codes", () => {
  beforeEach(() => {
    // The flow is a no-op in dev, which is the mode vitest runs in.
    vi.stubEnv("DEV", false);
    mocks.check.mockReset();
    mocks.trackUpdate.mockReset();
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "info").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
  });

  it("reports a missing platform entry as its own code, not as a failed check", async () => {
    mocks.check.mockRejectedValue(TARGET_NOT_FOUND);

    await updater().startBackgroundUpdateFlow();

    expect(mocks.trackUpdate).toHaveBeenCalledWith("failed", undefined, "update_target_missing");
    // A release that ships no artifact for this OS is not an incident.
    expect(console.error).not.toHaveBeenCalled();
  });

  it("reports an unreadable manifest apart from a dead endpoint", async () => {
    mocks.check.mockRejectedValue(
      new Error("Could not fetch a valid release JSON from the remote"),
    );

    await updater().startBackgroundUpdateFlow();

    expect(mocks.trackUpdate).toHaveBeenCalledWith("failed", undefined, "update_manifest_invalid");
  });

  it("still reports a network failure as check_failed", async () => {
    mocks.check.mockRejectedValue(new Error("error sending request for url (https://github.com)"));

    await updater().startBackgroundUpdateFlow();

    expect(mocks.trackUpdate).toHaveBeenCalledWith("failed", undefined, "check_failed");
    expect(console.error).toHaveBeenCalled();
  });

  it("stays quiet when there is no update", async () => {
    mocks.check.mockResolvedValue(null);

    await updater().startBackgroundUpdateFlow();

    expect(mocks.trackUpdate).not.toHaveBeenCalled();
  });
});

describe("classifyUpdateCheckError", () => {
  it("matches both shapes of the missing-target error", () => {
    expect(classifyUpdateCheckError(TARGET_NOT_FOUND)).toBe("update_target_missing");
    expect(classifyUpdateCheckError(TARGETS_NOT_FOUND)).toBe("update_target_missing");
  });

  it("matches the parse and signature errors the plugin forwards", () => {
    expect(classifyUpdateCheckError("missing field `version` at line 1 column 42")).toBe(
      "update_manifest_invalid",
    );
    expect(
      classifyUpdateCheckError(
        "The signature ...= could not be decoded, please check if it is a valid base64 string.",
      ),
    ).toBe("update_manifest_invalid");
  });

  it("falls back to check_failed for anything else", () => {
    expect(classifyUpdateCheckError("operation timed out")).toBe("check_failed");
    expect(classifyUpdateCheckError(undefined)).toBe("check_failed");
  });
});
