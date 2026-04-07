import { describe, it, expect } from "vitest";
import { matchesSearch } from "./useDisplayPipeline.svelte";
import type { PlatformAccount } from "$lib/shared/platform";

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
