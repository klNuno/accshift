import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Capture every backend call so we can assert when (and whether) a store is
// persisted. invoke is the only IPC surface clientStorage touches.
const invokeMock = vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(undefined));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// A full snapshot means initializeClientStorage never falls back to the
// legacy localStorage migration path, keeping the test free of jsdom storage.
const allStoreIds = [
  "client.settings",
  "client.folders",
  "client.account-card-notes",
  "client.account-card-colors",
  "client.account-default-game",
  "client.folder-card-colors",
  "client.view-mode",
  "cache.steam.profiles",
  "cache.roblox.profiles",
  "cache.steam.ban-check-state",
  "cache.steam.ban-info-cache",
];
const bootStores: Record<string, unknown> = {};
for (const id of allStoreIds) bootStores[id] = {};
vi.mock("$lib/app/bootPayload", () => ({
  getBootPayload: () => ({
    manifest: { schemaVersion: 1, stores: {} },
    storageSnapshot: {
      manifest: { schemaVersion: 1, stores: {} },
      stores: bootStores,
    },
  }),
}));

import {
  CLIENT_STORE_FOLDERS,
  initializeClientStorage,
  setClientStoreValue,
  flushPendingSaves,
  getClientStoreValue,
} from "./clientStorage";

function saveCalls() {
  return invokeMock.mock.calls.filter((call) => call[0] === "save_client_storage_store");
}

describe("clientStorage flushPendingSaves", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    invokeMock.mockClear();
    await initializeClientStorage();
    invokeMock.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("debounces a save instead of persisting immediately", () => {
    setClientStoreValue(CLIENT_STORE_FOLDERS, { a: 1 });
    // Memory is updated synchronously, but disk is not touched yet.
    expect(getClientStoreValue(CLIENT_STORE_FOLDERS)).toEqual({ a: 1 });
    expect(saveCalls()).toHaveLength(0);
  });

  it("flushPendingSaves persists pending stores immediately and cancels the timer", async () => {
    setClientStoreValue(CLIENT_STORE_FOLDERS, { folders: ["x"] });
    expect(saveCalls()).toHaveLength(0);

    await flushPendingSaves();

    const calls = saveCalls();
    expect(calls).toHaveLength(1);
    expect(calls[0][1]).toMatchObject({
      storeId: CLIENT_STORE_FOLDERS,
      value: { folders: ["x"] },
    });

    // The debounce timer must be gone: advancing past it persists nothing more.
    invokeMock.mockClear();
    vi.advanceTimersByTime(500);
    expect(saveCalls()).toHaveLength(0);
  });

  it("flushPendingSaves is a no-op when nothing is pending", async () => {
    await flushPendingSaves();
    expect(saveCalls()).toHaveLength(0);
  });

  it("flushPendingSaves waits for a save already started by the timer", async () => {
    let releaseSave: (() => void) | undefined;
    const saveStarted = new Promise<void>((resolve) => {
      invokeMock.mockImplementationOnce((command: unknown): Promise<unknown> => {
        if (command !== "save_client_storage_store") return Promise.resolve(undefined);
        resolve();
        return new Promise((release) => {
          releaseSave = () => release(undefined);
        });
      });
    });

    setClientStoreValue(CLIENT_STORE_FOLDERS, { folders: ["slow"] });
    vi.advanceTimersByTime(120);
    await saveStarted;

    let flushed = false;
    const flushPromise = flushPendingSaves().then(() => {
      flushed = true;
    });
    await Promise.resolve();
    expect(flushed).toBe(false);

    releaseSave?.();
    await flushPromise;
    expect(flushed).toBe(true);
    expect(saveCalls()).toHaveLength(1);
  });
});
