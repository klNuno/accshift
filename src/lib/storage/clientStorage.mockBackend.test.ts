import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The legacy localStorage migration reads the real user's keys. Under the mock
// backend (`pnpm dev:mock`) the app runs in the same webview profile, so the
// migration would put that data on screen and keep it in memory.
const mocks = vi.hoisted(() => ({ mockBackend: false }));

const invokeMock = vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(undefined));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// A snapshot with no stores at all: every store is a migration candidate.
vi.mock("$lib/app/bootPayload", () => ({
  getBootPayload: () => ({
    storageSnapshot: { manifest: { schemaVersion: 1, stores: {} }, stores: {} },
  }),
}));

vi.mock("./mockBackend", () => ({
  isMockBackend: () => mocks.mockBackend,
}));

const legacyFolders = { folders: [{ id: "real", name: "Real folder" }] };
const getItem = vi.fn((key: string) =>
  key === "accshift_folders" ? JSON.stringify(legacyFolders) : null,
);

async function initFreshStorage() {
  vi.resetModules();
  const storage = await import("./clientStorage");
  await storage.initializeClientStorage();
  return storage;
}

function saveCalls() {
  return invokeMock.mock.calls.filter((call) => call[0] === "save_client_storage_store");
}

describe("legacy localStorage migration", () => {
  beforeEach(() => {
    invokeMock.mockClear();
    getItem.mockClear();
    vi.stubGlobal("localStorage", { getItem });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("migrates legacy keys against the real backend", async () => {
    mocks.mockBackend = false;
    const storage = await initFreshStorage();

    expect(storage.getClientStoreValue(storage.CLIENT_STORE_FOLDERS)).toEqual(legacyFolders);
    expect(saveCalls().map((call) => (call[1] as { storeId: string }).storeId)).toContain(
      storage.CLIENT_STORE_FOLDERS,
    );
  });

  it("never reads legacy keys under the mock backend", async () => {
    mocks.mockBackend = true;
    const storage = await initFreshStorage();

    expect(getItem).not.toHaveBeenCalled();
    expect(storage.getClientStoreValue(storage.CLIENT_STORE_FOLDERS)).toBeUndefined();
    expect(saveCalls()).toHaveLength(0);
  });
});
