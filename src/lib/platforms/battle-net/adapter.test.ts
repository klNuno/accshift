import { describe, it, expect } from "vitest";
import { toBattleNetAccount } from "./adapter";

describe("battle.net account mapping", () => {
  it("converts the backend millisecond stamp to seconds", () => {
    const account = toBattleNetAccount({
      email: "player@example.com",
      battleTag: "Player#1234",
      lastLoginAt: 1_760_000_000_999,
    });
    expect(account.lastLoginAtSec).toBe(1_760_000_000);
  });

  // Listing an account is no longer a use, so the backend hands over an
  // account it has only ever seen with no stamp at all. Both shapes of "no
  // stamp" have to read as unknown rather than as the epoch.
  it("keeps a missing stamp as null", () => {
    expect(toBattleNetAccount({ email: "player@example.com" }).lastLoginAtSec).toBeNull();
    expect(
      toBattleNetAccount({ email: "player@example.com", lastLoginAt: null }).lastLoginAtSec,
    ).toBeNull();
  });

  it("maps a discovered account with no metadata at all", () => {
    const account = toBattleNetAccount({
      email: "fresh@example.com",
      battleTag: "",
      lastLoginAt: null,
    });
    expect(account.displayName).toBe("fresh");
    expect(account.lastLoginAtSec).toBeNull();
  });

  it("prefers the battle tag name over the email local part", () => {
    const account = toBattleNetAccount({ email: "player@example.com", battleTag: "Nova#1234" });
    expect(account.displayName).toBe("Nova");
    expect(account.id).toBe("player@example.com");
  });
});
