import { describe, it, expect } from "vitest";
import {
  collectVisibleAccountIds,
  matchesSearch,
  sectionCollapseKey,
  type DisplaySection,
} from "./useDisplayPipeline.svelte";
import type { PlatformAccount } from "$lib/shared/platform";
import type { FolderInfo, ItemRef } from "$lib/features/folders/types";

function makeAccount(overrides: Partial<PlatformAccount> = {}): PlatformAccount {
  return {
    id: "acc-1",
    username: "testuser",
    displayName: "Test User",
    ...overrides,
  };
}

describe("matchesSearch", () => {
  it("matches by ID", () => {
    expect(matchesSearch(makeAccount({ id: "user123" }), "user123")).toBe(true);
  });

  it("matches by username", () => {
    expect(matchesSearch(makeAccount({ username: "alice" }), "alice")).toBe(true);
  });

  it("matches by displayName", () => {
    expect(matchesSearch(makeAccount({ displayName: "Bob Smith" }), "bob")).toBe(true);
  });

  it("is case insensitive", () => {
    expect(matchesSearch(makeAccount({ username: "Alice" }), "alice")).toBe(true);
  });

  it("returns false for no match", () => {
    expect(matchesSearch(makeAccount(), "zzzzz")).toBe(false);
  });

  it("handles empty displayName", () => {
    expect(
      matchesSearch(makeAccount({ displayName: "", username: "xyz", id: "xyz" }), "test"),
    ).toBe(false);
  });

  it("matches partial strings", () => {
    expect(matchesSearch(makeAccount({ id: "longid12345" }), "id123")).toBe(true);
  });
});

const account = (id: string): ItemRef => ({ type: "account", id });

function section(folderId: string | null, accountIds: string[]): DisplaySection {
  return {
    folder: folderId ? ({ id: folderId, name: folderId } as FolderInfo) : null,
    folderItems: [],
    accountItems: accountIds.map(account),
  };
}

describe("collectVisibleAccountIds", () => {
  it("leaves out accounts of collapsed sections", () => {
    const sections = [section(null, ["r1", "r2"]), section("f1", ["a1"]), section("f2", ["b1"])];
    const collapsed = new Set(["f1"]);

    expect(collectVisibleAccountIds(sections, [], collapsed)).toEqual(["r1", "r2", "b1"]);
  });

  it("collapses the root section by its own key", () => {
    const sections = [section(null, ["r1"]), section("f1", ["a1"])];
    const collapsed = new Set([sectionCollapseKey(sections[0])]);

    expect(collectVisibleAccountIds(sections, [], collapsed)).toEqual(["a1"]);
  });

  it("uses the flat list outside sections mode and drops duplicates", () => {
    const items: ItemRef[] = [
      account("a1"),
      { type: "folder", id: "f1" },
      account("a1"),
      account("a2"),
    ];

    expect(collectVisibleAccountIds(null, items, new Set(["f1"]))).toEqual(["a1", "a2"]);
  });
});
