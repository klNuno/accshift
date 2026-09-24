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
  CLIENT_STORE_SETTINGS,
  initializeClientStorage,
  setClientStoreValue,
  flushPendingSaves,
  getClientStoreValue,
  onClientStoreChange,
  refreshClientStorageIfChanged,
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

  it("marks its own write as seen so the next focus finds no change", async () => {
    // Backend answers the post-write fingerprint; the following manifest
    // reports exactly that fingerprint, so nothing looks external.
    invokeMock.mockImplementationOnce(() => Promise.resolve("file:42:4242"));
    invokeMock.mockImplementationOnce(() =>
      Promise.resolve({
        schemaVersion: 1,
        stores: { [CLIENT_STORE_FOLDERS]: "file:42:4242" },
      }),
    );

    setClientStoreValue(CLIENT_STORE_FOLDERS, { folders: ["x"] });
    await flushPendingSaves();

    const changed = await refreshClientStorageIfChanged();
    expect(changed).toEqual([]);
    // No full snapshot reload was triggered by our own write.
    expect(
      invokeMock.mock.calls.filter((call) => call[0] === "load_client_storage_snapshot"),
    ).toHaveLength(0);
  });
});

describe("clientStorage external refresh", () => {
  // What the next manifest and snapshot calls answer. Each test sets its own.
  let manifest: Record<string, string> = {};
  let snapshotManifest: Record<string, string> = {};
  let snapshotStores: Record<string, unknown> = {};
  let fingerprint = 0;

  beforeEach(async () => {
    vi.useFakeTimers();
    await initializeClientStorage();
    // Settle anything a previous test left pending, then record every store as
    // seen so each test starts from a clean manifest.
    await flushPendingSaves();
    invokeMock.mockImplementation((command: unknown): Promise<unknown> => {
      if (command === "get_storage_manifest") {
        return Promise.resolve({ schemaVersion: 1, stores: manifest });
      }
      if (command === "load_client_storage_snapshot") {
        return Promise.resolve({
          manifest: { schemaVersion: 1, stores: snapshotManifest },
          stores: snapshotStores,
        });
      }
      if (command === "save_client_storage_store") {
        fingerprint += 1;
        return Promise.resolve(`local:${fingerprint}`);
      }
      return Promise.resolve(undefined);
    });
    manifest = { [CLIENT_STORE_FOLDERS]: "seen", [CLIENT_STORE_SETTINGS]: "seen" };
    snapshotManifest = manifest;
    snapshotStores = {};
    await refreshClientStorageIfChanged();
    invokeMock.mockClear();
  });

  afterEach(() => {
    invokeMock.mockImplementation(() => Promise.resolve(undefined));
    vi.useRealTimers();
  });

  it("keeps an edit still in the save debounce and saves it afterwards", async () => {
    setClientStoreValue(CLIENT_STORE_FOLDERS, { folders: ["local"] });

    manifest = { [CLIENT_STORE_FOLDERS]: "external", [CLIENT_STORE_SETTINGS]: "seen" };
    snapshotManifest = manifest;
    snapshotStores = { [CLIENT_STORE_FOLDERS]: { folders: ["external"] } };
    await refreshClientStorageIfChanged();

    expect(getClientStoreValue(CLIENT_STORE_FOLDERS)).toEqual({ folders: ["local"] });

    await vi.advanceTimersByTimeAsync(120);
    const calls = saveCalls();
    expect(calls).toHaveLength(1);
    expect(calls[0][1]).toMatchObject({
      storeId: CLIENT_STORE_FOLDERS,
      value: { folders: ["local"] },
    });
  });

  it("keeps an edit whose save is still in flight", async () => {
    let releaseSave: (() => void) | undefined;
    const defaultImpl = invokeMock.getMockImplementation()!;
    invokeMock.mockImplementation((command: unknown, ...rest: unknown[]) => {
      if (command !== "save_client_storage_store") return defaultImpl(command, ...rest);
      return new Promise((release) => {
        releaseSave = () => release("local:slow");
      });
    });

    setClientStoreValue(CLIENT_STORE_FOLDERS, { folders: ["local"] });
    await vi.advanceTimersByTimeAsync(120);
    expect(releaseSave).toBeDefined();

    manifest = { [CLIENT_STORE_FOLDERS]: "external", [CLIENT_STORE_SETTINGS]: "seen" };
    snapshotManifest = manifest;
    snapshotStores = { [CLIENT_STORE_FOLDERS]: { folders: ["external"] } };
    try {
      await refreshClientStorageIfChanged();
      expect(getClientStoreValue(CLIENT_STORE_FOLDERS)).toEqual({ folders: ["local"] });
    } finally {
      releaseSave?.();
      await flushPendingSaves();
    }
  });

  it("still applies external changes to stores with no pending save", async () => {
    setClientStoreValue(CLIENT_STORE_FOLDERS, { folders: ["local"] });

    manifest = { [CLIENT_STORE_FOLDERS]: "external", [CLIENT_STORE_SETTINGS]: "external" };
    snapshotManifest = manifest;
    snapshotStores = {
      [CLIENT_STORE_FOLDERS]: { folders: ["external"] },
      [CLIENT_STORE_SETTINGS]: { language: "fr" },
    };
    const changed = await refreshClientStorageIfChanged();

    expect(changed).toEqual([CLIENT_STORE_SETTINGS]);
    expect(getClientStoreValue(CLIENT_STORE_SETTINGS)).toEqual({ language: "fr" });
    expect(getClientStoreValue(CLIENT_STORE_FOLDERS)).toEqual({ folders: ["local"] });
    await flushPendingSaves();
  });

  it("does not mark a store as seen when it changed after the manifest read", async () => {
    // The first manifest only reports folders; settings is rewritten between
    // the manifest read and the snapshot read, so the snapshot manifest
    // already carries it. It must still be picked up by the next refresh.
    manifest = { [CLIENT_STORE_FOLDERS]: "external", [CLIENT_STORE_SETTINGS]: "seen" };
    snapshotManifest = { [CLIENT_STORE_FOLDERS]: "external", [CLIENT_STORE_SETTINGS]: "later" };
    snapshotStores = {
      [CLIENT_STORE_FOLDERS]: { folders: ["external"] },
      [CLIENT_STORE_SETTINGS]: { language: "de" },
    };
    expect(await refreshClientStorageIfChanged()).toEqual([CLIENT_STORE_FOLDERS]);

    manifest = snapshotManifest;
    expect(await refreshClientStorageIfChanged()).toEqual([CLIENT_STORE_SETTINGS]);
    expect(getClientStoreValue(CLIENT_STORE_SETTINGS)).toEqual({ language: "de" });
  });
});

describe("clientStorage onClientStoreChange", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    await initializeClientStorage();
  });

  afterEach(async () => {
    await flushPendingSaves();
    vi.useRealTimers();
  });

  it("tells a listener about writes to its store only, until it unsubscribes", () => {
    const seen: unknown[] = [];
    const stop = onClientStoreChange(CLIENT_STORE_SETTINGS, () => {
      seen.push(getClientStoreValue(CLIENT_STORE_SETTINGS));
    });

    setClientStoreValue(CLIENT_STORE_FOLDERS, { a: 1 });
    setClientStoreValue(CLIENT_STORE_SETTINGS, { uiScalePercent: 110 });
    expect(seen).toEqual([{ uiScalePercent: 110 }]);

    stop();
    setClientStoreValue(CLIENT_STORE_SETTINGS, { uiScalePercent: 90 });
    expect(seen).toHaveLength(1);
  });

  it("keeps the write and the other listeners when one listener throws", () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    let calls = 0;
    const stopBroken = onClientStoreChange(CLIENT_STORE_SETTINGS, () => {
      throw new Error("boom");
    });
    const stop = onClientStoreChange(CLIENT_STORE_SETTINGS, () => {
      calls += 1;
    });

    setClientStoreValue(CLIENT_STORE_SETTINGS, { language: "fr" });
    expect(getClientStoreValue(CLIENT_STORE_SETTINGS)).toEqual({ language: "fr" });
    expect(calls).toBe(1);
    expect(errors).toHaveBeenCalledTimes(1);

    stopBroken();
    stop();
    errors.mockRestore();
  });
});
