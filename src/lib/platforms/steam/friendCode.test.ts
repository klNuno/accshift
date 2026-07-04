import { describe, it, expect } from "vitest";
import { encodeFriendCode } from "./friendCode";

describe("encodeFriendCode", () => {
  it("matches the known-correct CS2 friend code for a real SteamID64", () => {
    // Test vector from the reference implementation (emily33901/js-csfriendcode):
    // 76561197960287930 -> SUCVS-FADA.
    expect(encodeFriendCode("76561197960287930")).toBe("SUCVS-FADA");
  });

  it("is stable across repeated calls for the same id", () => {
    const id = "76561198123456789";
    expect(encodeFriendCode(id)).toBe(encodeFriendCode(id));
  });

  it("returns an empty string instead of throwing on a non-numeric id", () => {
    expect(() => encodeFriendCode("some-junk-key")).not.toThrow();
    expect(encodeFriendCode("some-junk-key")).toBe("");
  });

  it('does not throw on an empty id (BigInt("") coerces to 0n)', () => {
    expect(() => encodeFriendCode("")).not.toThrow();
    expect(encodeFriendCode("")).toBe(encodeFriendCode("0"));
  });

  it("returns an empty string instead of throwing on a decimal id", () => {
    expect(() => encodeFriendCode("1.5")).not.toThrow();
    expect(encodeFriendCode("1.5")).toBe("");
  });
});
