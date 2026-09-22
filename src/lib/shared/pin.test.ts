import { describe, it, expect } from "vitest";
import { sanitizePinDigits, isValidPinHash, hashPinCode, verifyPinCode } from "./pin";

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

async function legacySha256(pin: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(pin));
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

describe("verifyPinCode", () => {
  it("verifies correct PIN against PBKDF2 hash", async () => {
    const hash = await hashPinCode("5678");
    expect(await verifyPinCode("5678", hash)).toEqual({ matches: true, rehashed: null });
  });

  it("rejects wrong PIN", async () => {
    const hash = await hashPinCode("5678");
    expect(await verifyPinCode("0000", hash)).toEqual({ matches: false, rehashed: null });
  });

  it("handles legacy SHA-256 hash", async () => {
    const legacyHash = await legacySha256("1234");
    expect((await verifyPinCode("1234", legacyHash)).matches).toBe(true);
    expect((await verifyPinCode("0000", legacyHash)).matches).toBe(false);
  });

  it("handles uppercase stored hashes", async () => {
    const hash = await hashPinCode("5678");
    expect((await verifyPinCode("5678", hash.toUpperCase())).matches).toBe(true);
  });

  it("rejects invalid PIN length", async () => {
    const hash = await hashPinCode("1234");
    expect(await verifyPinCode("12", hash)).toEqual({ matches: false, rehashed: null });
  });
});

// F-12: the unsalted form used to be accepted for ever because nothing ever
// rewrote it. A legacy hash must survive exactly one unlock.
describe("legacy hash migration", () => {
  it("returns a PBKDF2 replacement after a legacy hash verifies", async () => {
    const legacyHash = await legacySha256("1234");
    const { matches, rehashed } = await verifyPinCode("1234", legacyHash);

    expect(matches).toBe(true);
    expect(rehashed).toMatch(/^[a-f0-9]{32}:[a-f0-9]{64}$/);
    // The replacement takes the same PIN, and it is not the legacy form.
    expect(rehashed).not.toBe(legacyHash);
    expect((await verifyPinCode("1234", rehashed as string)).matches).toBe(true);
    expect((await verifyPinCode("0000", rehashed as string)).matches).toBe(false);
  });

  it("rewrites nothing when the code is wrong", async () => {
    const legacyHash = await legacySha256("1234");
    expect(await verifyPinCode("0000", legacyHash)).toEqual({ matches: false, rehashed: null });
  });

  it("rewrites nothing for a hash that is already PBKDF2", async () => {
    const hash = await hashPinCode("1234");
    expect(await verifyPinCode("1234", hash)).toEqual({ matches: true, rehashed: null });
  });

  it("replaces the hash once, so the second unlock has nothing left to migrate", async () => {
    const legacyHash = await legacySha256("1234");
    const first = await verifyPinCode("1234", legacyHash);
    const second = await verifyPinCode("1234", first.rehashed as string);

    expect(second).toEqual({ matches: true, rehashed: null });
  });
});

// Locked against the same literals in `crates/accshift-cli/src/pin.rs`
// (`gui_cross_check_vector_verifies`). A hash written on either side must
// verify on the other: the CLI reads the very file the GUI writes.
describe("CLI interoperability", () => {
  const CROSS_CHECK_SALT_HEX = "000102030405060708090a0b0c0d0e0f";
  const CROSS_CHECK_HASH_HEX = "e19d9507e40b77fbb7503faedce7cb4ebf8c6820a8b746d9dfa9fcab899ec65d";
  const CROSS_CHECK_PIN = "4321";

  it("verifies the shared PBKDF2 vector", async () => {
    const stored = `${CROSS_CHECK_SALT_HEX}:${CROSS_CHECK_HASH_HEX}`;
    expect((await verifyPinCode(CROSS_CHECK_PIN, stored)).matches).toBe(true);
    expect((await verifyPinCode("1111", stored)).matches).toBe(false);
  });

  it("verifies the shared legacy SHA-256 vector", async () => {
    // SHA-256 of the digits "1234", the vector pinned in the Rust tests.
    const legacy = "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4";
    expect(await legacySha256("1234")).toBe(legacy);
    expect((await verifyPinCode("1234", legacy)).matches).toBe(true);
  });
});
