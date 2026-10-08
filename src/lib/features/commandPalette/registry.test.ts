import { describe, expect, it } from "vitest";
import type { PlatformAccount } from "$lib/shared/platform";
import { createCommandRegistry } from "./registry";

const noop = () => {};

function registry(accounts: PlatformAccount[], currentId: string | null) {
  return createCommandRegistry({
    t: (key) => key,
    getAccounts: () => accounts,
    getCurrentAccountId: () => currentId,
    getEnabledPlatforms: () => [],
    getUnavailablePlatformIds: () => new Set(),
    getActiveTab: () => "steam",
    getActiveTabUsable: () => true,
    getCurrentFolders: () => [],
    getCurrentFolderId: () => null,
    isBulkEditAvailable: () => false,
    isPersonasEnabled: () => false,
    getUpdateCtaLabel: () => "",
    getViewMode: () => "grid",
    isMac: () => false,
    switchToAccount: noop,
    addAccount: noop,
    refreshAccounts: noop,
    newFolder: noop,
    openFolder: noop,
    navigateToParent: noop,
    changeTab: noop,
    toggleSettings: noop,
    openPersonas: noop,
    toggleBulkEdit: noop,
    toggleViewMode: noop,
    zoomReset: noop,
    applyUpdate: noop,
  });
}

describe("account commands", () => {
  const accounts: PlatformAccount[] = [
    { id: "a", displayName: "main", username: "main_login" },
    { id: "b", displayName: "alt", username: "alt_login" },
  ];

  it("marks the current account active once, through the badge alone", () => {
    const all = registry(accounts, "a").getCommands();
    const commands = all.filter((command) => command.section === "accounts");

    const byId = (id: string) => {
      const command = commands.find((entry) => entry.id === id);
      if (!command) throw new Error(`missing command ${id}`);
      return command;
    };

    // The palette renders both `active` (as a badge) and `hint`, so a hint
    // saying "Active" too would print the word twice on the same row.
    expect(byId("account:a").active).toBe(true);
    expect(byId("account:b").active).toBeFalsy();
    for (const command of commands) expect(command.hint).toBeUndefined();
    // Only the account rows lose their hint: the shortcuts stay on the actions.
    expect(all.find((command) => command.id === "action:add-account")?.hint).toMatch(/\+N$/);
  });
});
