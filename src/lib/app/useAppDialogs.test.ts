import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FolderInfo } from "$lib/features/folders/types";

const mocks = vi.hoisted(() => ({ deleteFolder: vi.fn() }));

vi.mock("$lib/features/folders/store", () => ({
  createFolder: vi.fn(),
  deleteFolder: (...args: unknown[]) => mocks.deleteFolder(...args),
  findItemFolderId: vi.fn(),
  getFolderPath: () => [],
  listFolders: () => [],
  moveItem: vi.fn(),
  renameFolder: vi.fn(),
}));

import { createAppDialogsController } from "./useAppDialogs.svelte";

function createDialogs() {
  return createAppDialogsController({
    t: (key) => key,
    getAdapter: () => undefined,
    getActiveTab: () => "steam",
    getActiveTabUsable: () => true,
    getCurrentFolderId: () => null,
    getCurrentAccountId: () => null,
    refreshCurrentItems: vi.fn(),
    loadAccounts: vi.fn(),
    removeAccount: vi.fn(),
    getAccountCardColor: () => "",
    getAccountNote: () => "",
    getFolderCardColor: () => "",
    getColorLabel: (id) => id,
    copyToClipboard: vi.fn(),
    showToast: vi.fn(),
    bumpCardColorVersion: vi.fn(),
    bumpCardNoteVersion: vi.fn(),
  });
}

const folder = { id: "f1", name: "Smurfs" } as FolderInfo;
const click = { clientX: 0, clientY: 0 } as MouseEvent;

function openDeleteFolderConfirm(dialogs: ReturnType<typeof createDialogs>) {
  dialogs.openFolderContextMenu(click, folder);
  const deleteItem = dialogs.contextMenuItems.find(
    (item) => "label" in item && item.label === "context.menu.deleteFolder",
  );
  if (!deleteItem || !("action" in deleteItem) || !deleteItem.action) {
    throw new Error("delete folder item missing");
  }
  deleteItem.action();
}

describe("confirm dialog replacement", () => {
  beforeEach(() => {
    mocks.deleteFolder.mockReset();
  });

  it("settles a pending confirm with false when another confirm replaces it", async () => {
    const dialogs = createDialogs();
    const pending = dialogs.requestConfirm({ title: "switch", message: "sure?" });

    openDeleteFolderConfirm(dialogs);
    const answered = await Promise.race([
      pending,
      new Promise((done) => setTimeout(() => done("unsettled"), 0)),
    ]);

    expect(answered).toBe(false);
  });

  it("confirming the replacing dialog runs its action and never accepts the old request", async () => {
    const dialogs = createDialogs();
    const results: boolean[] = [];
    void dialogs.requestConfirm({ title: "switch", message: "sure?" }).then((v) => results.push(v));

    openDeleteFolderConfirm(dialogs);
    dialogs.confirmCurrentDialog();
    await Promise.resolve();

    expect(mocks.deleteFolder).toHaveBeenCalledWith("f1");
    expect(results).toEqual([false]);
  });

  it("still resolves true when the request's own dialog is confirmed", async () => {
    const dialogs = createDialogs();
    const pending = dialogs.requestConfirm({ title: "switch", message: "sure?" });

    dialogs.confirmCurrentDialog();

    await expect(pending).resolves.toBe(true);
  });
});
