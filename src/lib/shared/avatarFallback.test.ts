import { describe, it, expect } from "vitest";
import { getAvatarInitials, getAvatarSeed, getAvatarGradientStyle } from "./avatarFallback";

describe("getAvatarInitials", () => {
  it("returns two initials for two words", () => {
    expect(getAvatarInitials("John Doe")).toBe("JD");
  });

  it("returns one initial for single word", () => {
    expect(getAvatarInitials("Alice")).toBe("A");
  });

  it("returns ? for empty string", () => {
    expect(getAvatarInitials("")).toBe("?");
  });

  it("returns ? for whitespace only", () => {
    expect(getAvatarInitials("   ")).toBe("?");
  });

  it("uppercases initials", () => {
    expect(getAvatarInitials("john doe")).toBe("JD");
  });

  it("handles multiple spaces between words", () => {
    expect(getAvatarInitials("John   Doe")).toBe("JD");
  });
});

describe("getAvatarSeed", () => {
  it("uses displayName when available", () => {
    const seed = getAvatarSeed("Alice", "alice_user", "123");
    expect(seed).toContain("Alice");
    expect(seed).toContain("123");
  });

  it("falls back to username", () => {
    const seed = getAvatarSeed("", "bob_user", "456");
    expect(seed).toContain("bob_user");
  });

  it("is deterministic", () => {
    const a = getAvatarSeed("X", "Y", "Z");
    const b = getAvatarSeed("X", "Y", "Z");
    expect(a).toBe(b);
  });

  it("produces different seeds for different IDs", () => {
    const a = getAvatarSeed("Same", "same", "111");
    const b = getAvatarSeed("Same", "same", "222");
    expect(a).not.toBe(b);
  });
});

describe("getAvatarGradientStyle", () => {
  it("returns CSS with background-color and background-image", () => {
    const style = getAvatarGradientStyle("test-seed");
    expect(style).toContain("background-color:hsl(");
    expect(style).toContain("background-image:linear-gradient(");
  });

  it("is deterministic", () => {
    expect(getAvatarGradientStyle("x")).toBe(getAvatarGradientStyle("x"));
  });

  it("varies with different seeds", () => {
    expect(getAvatarGradientStyle("a")).not.toBe(getAvatarGradientStyle("b"));
  });
});
