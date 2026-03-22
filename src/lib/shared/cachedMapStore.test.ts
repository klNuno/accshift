import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("$lib/storage/clientStorage", () => {
  let stores = new Map<string, unknown>();
  let revisions = new Map<string, number>();
  return {
    getClientStoreValue: (id: string) => stores.get(id) ?? null,
    setClientStoreValue: (id: string, value: unknown) => {
      stores.set(id, value);
      revisions.set(id, (revisions.get(id) ?? 0) + 1);
    },
    getClientStoreRevision: (id: string) => revisions.get(id) ?? 0,
  };
});

import { createCachedMapStore } from "./cachedMapStore";
import type { ClientStoreId } from "$lib/storage/clientStorage";

const TEST_STORE_ID = "client.account-card-notes" as ClientStoreId;

function makeStore(sanitize: (key: string, value: string) => string | null = (_k, v) => v) {
  return createCachedMapStore(TEST_STORE_ID, sanitize);
}

describe("createCachedMapStore", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it("creates a store with sanitizeEntry that accepts valid entries", async () => {
    vi.resetModules();
    const mod = await import("./cachedMapStore");
    const store = mod.createCachedMapStore(TEST_STORE_ID, (_k, v) => (v.length > 0 ? v : null));

    store.set("a", "hello");
    expect(store.get("a")).toBe("hello");
  });

  it("get returns empty string for unknown keys", () => {
    const store = makeStore();
    expect(store.get("nonexistent")).toBe("");
  });

  it("set stores and retrieves values", () => {
    const store = makeStore();
    store.set("acc1", "blue");
    expect(store.get("acc1")).toBe("blue");
  });

  it("set with null sanitization result removes the entry", () => {
    const store = makeStore((_k, v) => (v === "bad" ? null : v));

    store.set("acc1", "good");
    expect(store.get("acc1")).toBe("good");

    store.set("acc1", "bad");
    expect(store.get("acc1")).toBe("");
  });

  it("remove deletes entries", () => {
    const store = makeStore();
    store.set("acc1", "value");
    expect(store.get("acc1")).toBe("value");

    store.remove("acc1");
    expect(store.get("acc1")).toBe("");
  });
});
