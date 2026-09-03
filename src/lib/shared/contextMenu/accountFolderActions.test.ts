import { describe, expect, it, vi } from "vitest";
import { getAccountFolderContextActions } from "./accountFolderActions";
import type { AccountFolderActionCallbacks } from "./accountFolderActions";
import type { ContextMenuItemAction } from "./types";
import type { PlatformAccount } from "../platform";

const ACCOUNT = { id: "acc-1", displayName: "Main", username: "main" } as PlatformAccount;

function callbacks(overrides: Partial<AccountFolderActionCallbacks> = {}) {
  return {
    t: (key: string) => key,
    getFolders: () => [
      { id: "f1", label: "Smurfs" },
      { id: "f2", label: "Smurfs / Old" },
    ],
    getCurrentFolderId: () => null,
    moveToFolder: vi.fn(),
    ...overrides,
  } as AccountFolderActionCallbacks;
}

function submenuOf(
  actions: ReturnType<typeof getAccountFolderContextActions>,
): ContextMenuItemAction[] {
  const entry = actions[0] as ContextMenuItemAction;
  return (entry.submenu ?? []) as ContextMenuItemAction[];
}

describe("move to folder context action", () => {
  it("lists every folder when the account is at the root", () => {
    const actions = getAccountFolderContextActions(ACCOUNT, callbacks());
    expect(actions).toHaveLength(1);
    expect(submenuOf(actions).map((item) => item.label)).toEqual(["Smurfs", "Smurfs / Old"]);
  });

  it("offers the root and hides the folder the account is already in", () => {
    const actions = getAccountFolderContextActions(
      ACCOUNT,
      callbacks({ getCurrentFolderId: () => "f1" }),
    );
    expect(submenuOf(actions).map((item) => item.label)).toEqual([
      "context.menu.moveToRoot",
      "Smurfs / Old",
    ]);
  });

  it("shows nothing when there is no folder and the account is at the root", () => {
    const actions = getAccountFolderContextActions(ACCOUNT, callbacks({ getFolders: () => [] }));
    expect(actions).toEqual([]);
  });

  it("keeps the root as the only way out of the single folder", () => {
    const actions = getAccountFolderContextActions(
      ACCOUNT,
      callbacks({
        getFolders: () => [{ id: "f1", label: "Smurfs" }],
        getCurrentFolderId: () => "f1",
      }),
    );
    expect(submenuOf(actions).map((item) => item.label)).toEqual(["context.menu.moveToRoot"]);
  });

  it("moves to the picked folder, and to null for the root", () => {
    const moveToFolder = vi.fn();
    const actions = getAccountFolderContextActions(
      ACCOUNT,
      callbacks({ getCurrentFolderId: () => "f1", moveToFolder }),
    );
    const submenu = submenuOf(actions);
    submenu[0].action?.();
    expect(moveToFolder).toHaveBeenLastCalledWith(null);
    submenu[1].action?.();
    expect(moveToFolder).toHaveBeenLastCalledWith("f2");
  });
});
