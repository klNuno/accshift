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

import { createFolder, findItemFolderId, getItemsInFolder, listFolders, moveItem } from "./store";

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

describe("moving an account without a drag", () => {
  const STORE = {
    version: 1,
    folders: [
      { id: "smurfs", name: "Smurfs", parentId: null, platform: "steam" },
      { id: "old", name: "Old", parentId: "smurfs", platform: "steam" },
      { id: "riot-folder", name: "Ranked", parentId: null, platform: "riot" },
    ],
    itemOrder: {
      "root:steam": [
        { type: "folder", id: "smurfs" },
        { type: "account", id: "at-root" },
      ],
      smurfs: [
        { type: "folder", id: "old" },
        { type: "account", id: "in-smurfs" },
      ],
      old: [{ type: "account", id: "in-old" }],
      "root:riot": [{ type: "folder", id: "riot-folder" }],
    },
  };

  beforeEach(() => {
    loadStore(STORE);
  });

  it("lists only the folders of the active platform", () => {
    expect(listFolders("steam").map((folder) => folder.id)).toEqual(["smurfs", "old"]);
    expect(listFolders("riot").map((folder) => folder.id)).toEqual(["riot-folder"]);
  });

  it("finds the folder holding an account, nested one included", () => {
    expect(findItemFolderId({ type: "account", id: "in-smurfs" }, "steam")).toBe("smurfs");
    expect(findItemFolderId({ type: "account", id: "in-old" }, "steam")).toBe("old");
  });

  it("reads an account at the platform root as no folder", () => {
    expect(findItemFolderId({ type: "account", id: "at-root" }, "steam")).toBeNull();
    expect(findItemFolderId({ type: "account", id: "unknown" }, "steam")).toBeNull();
  });

  it("puts a root account into a folder, the same move a drop performs", () => {
    moveItem({ type: "account", id: "at-root" }, null, "old", "steam");
    expect(getItemsInFolder("old", "steam")).toEqual([
      { type: "account", id: "in-old" },
      { type: "account", id: "at-root" },
    ]);
    expect(getItemsInFolder(null, "steam")).toEqual([{ type: "folder", id: "smurfs" }]);
  });

  it("sends an account back to the root", () => {
    moveItem({ type: "account", id: "in-old" }, "old", null, "steam");
    expect(getItemsInFolder("old", "steam")).toEqual([]);
    expect(getItemsInFolder(null, "steam")).toEqual([
      { type: "folder", id: "smurfs" },
      { type: "account", id: "at-root" },
      { type: "account", id: "in-old" },
    ]);
  });
});
