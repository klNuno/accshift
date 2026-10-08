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
    const commands = registry(accounts, "a")
      .getCommands()
      .filter((command) => command.section === "accounts");

    const current = commands.find((command) => command.id === "account:a");
    const other = commands.find((command) => command.id === "account:b");

    // The palette renders both `active` (as a badge) and `hint`, so a hint
    // saying "Active" too would print the word twice on the same row.
    expect(current?.active).toBe(true);
    expect(current?.hint).toBeUndefined();
    expect(other?.active).toBe(false);
  });
});
