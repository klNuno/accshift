import { describe, it, expect } from "vitest";
import {
  sanitizePinDigits,
  isValidPinCode,
  isValidPinHash,
  hashPinCode,
  verifyPinCode,
} from "./pin";

describe("sanitizePinDigits", () => {
  it("removes non-digits", () => {
    expect(sanitizePinDigits("1a2b3c4d")).toBe("1234");
  });

  it("truncates to 4 digits", () => {
    expect(sanitizePinDigits("123456")).toBe("1234");
  });

  it("returns empty for no digits", () => {
    expect(sanitizePinDigits("abc")).toBe("");
  });

  it("handles empty string", () => {
    expect(sanitizePinDigits("")).toBe("");
  });
});

describe("isValidPinCode", () => {
  it("accepts 4 digits", () => {
    expect(isValidPinCode("1234")).toBe(true);
  });

  it("rejects 3 digits", () => {
    expect(isValidPinCode("123")).toBe(false);
  });

  it("rejects non-digits", () => {
    expect(isValidPinCode("abcd")).toBe(false);
  });
});

describe("isValidPinHash", () => {
  it("accepts PBKDF2 format (salt:hash)", () => {
    const salt = "a".repeat(32);
    const hash = "b".repeat(64);
    expect(isValidPinHash(`${salt}:${hash}`)).toBe(true);
  });

  it("accepts legacy SHA-256 format", () => {
    expect(isValidPinHash("a".repeat(64))).toBe(true);
  });

  it("rejects invalid format", () => {
    expect(isValidPinHash("invalid")).toBe(false);
  });

  it("rejects empty string", () => {
    expect(isValidPinHash("")).toBe(false);
  });
});

describe("hashPinCode", () => {
  it("returns salt:hash format for valid PIN", async () => {
    const result = await hashPinCode("1234");
    expect(result).toMatch(/^[a-f0-9]{32}:[a-f0-9]{64}$/i);
  });

  it("returns empty for invalid PIN", async () => {
    expect(await hashPinCode("12")).toBe("");
    expect(await hashPinCode("abc")).toBe("");
  });

  it("generates different salts each call", async () => {
    const a = await hashPinCode("1234");
    const b = await hashPinCode("1234");
    expect(a).not.toBe(b);
  });
});

describe("verifyPinCode", () => {
  it("verifies correct PIN against PBKDF2 hash", async () => {
    const hash = await hashPinCode("5678");
    expect(await verifyPinCode("5678", hash)).toBe(true);
  });

  it("rejects wrong PIN", async () => {
    const hash = await hashPinCode("5678");
    expect(await verifyPinCode("0000", hash)).toBe(false);
  });

  it("handles legacy SHA-256 hash", async () => {
    // SHA-256 of "1234"
    const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode("1234"));
    const legacyHash = Array.from(new Uint8Array(digest))
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
    expect(await verifyPinCode("1234", legacyHash)).toBe(true);
    expect(await verifyPinCode("0000", legacyHash)).toBe(false);
  });

  it("rejects invalid PIN length", async () => {
    const hash = await hashPinCode("1234");
    expect(await verifyPinCode("12", hash)).toBe(false);
  });
});
