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

  it("keeps a missing stamp as null", () => {
    expect(toBattleNetAccount({ email: "player@example.com" }).lastLoginAtSec).toBeNull();
    expect(
      toBattleNetAccount({ email: "player@example.com", lastLoginAt: null }).lastLoginAtSec,
    ).toBeNull();
  });

  it("prefers the battle tag name over the email local part", () => {
    const account = toBattleNetAccount({ email: "player@example.com", battleTag: "Nova#1234" });
    expect(account.displayName).toBe("Nova");
    expect(account.id).toBe("player@example.com");
  });
});
