import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FolderStore } from "./types";

const mocks = vi.hoisted(() => ({
  stored: null as unknown,
  revision: 0,
  saved: null as unknown,
}));

vi.mock("$lib/storage/clientStorage", () => ({
  CLIENT_STORE_FOLDERS: "client.folders",
  getClientStoreRevision: () => mocks.revision,
  getClientStoreValue: () => mocks.stored,
  setClientStoreValue: (_id: string, value: unknown) => {
    mocks.saved = value;
  },
}));

import { createFolder, getItemsInFolder } from "./store";

/** Replaces what the client store holds and invalidates the module's cache. */
function loadStore(value: unknown) {
  mocks.stored = value;
  mocks.saved = null;
  mocks.revision += 1;
}

describe("folder store sanitizing", () => {
  beforeEach(() => {
    loadStore(null);
  });

  it("drops an itemOrder bucket keyed by a folder that no longer exists", () => {
    loadStore({
      version: 1,
      folders: [{ id: "live", name: "Live", parentId: null, platform: "steam" }],
      itemOrder: {
        "root:steam": [{ type: "folder", id: "live" }],
        live: [{ type: "account", id: "account-1" }],
        ghost: [{ type: "account", id: "account-2" }],
      },
    });

    expect(getItemsInFolder("live", "steam")).toEqual([{ type: "account", id: "account-1" }]);
    // The bucket of the deleted folder is gone, not merely unreachable.
    expect(getItemsInFolder("ghost", "steam")).toEqual([]);

    // Any write persists the cleaned shape, so the orphan never comes back.
    createFolder("New", null, "steam");
    expect(Object.keys((mocks.saved as FolderStore).itemOrder).sort()).not.toContain("ghost");
  });

  it("keeps every platform root bucket, including platforms with no folder", () => {
    loadStore({
      version: 1,
      folders: [],
      itemOrder: {
        "root:steam": [{ type: "account", id: "account-1" }],
        "root:riot": [{ type: "account", id: "account-2" }],
      },
    });

    expect(getItemsInFolder(null, "steam")).toEqual([{ type: "account", id: "account-1" }]);
    expect(getItemsInFolder(null, "riot")).toEqual([{ type: "account", id: "account-2" }]);
  });

  it("drops a bare root prefix, which names no platform", () => {
    loadStore({
      version: 1,
      folders: [],
      itemOrder: {
        "root:": [{ type: "account", id: "account-1" }],
      },
    });

    createFolder("New", null, "steam");
    expect(Object.keys((mocks.saved as FolderStore).itemOrder)).not.toContain("root:");
  });
});
