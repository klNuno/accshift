import { describe, it, expect } from "vitest";
import { toRobloxAccount } from "./adapter";

describe("roblox account mapping", () => {
  it("converts the backend millisecond stamp to seconds", () => {
    const account = toRobloxAccount({
      userId: "1234",
      username: "player",
      displayName: "Player",
      lastLoginAt: 1_760_000_000_999,
    });
    expect(account.lastLoginAtSec).toBe(1_760_000_000);
  });

  it("keeps a missing stamp as null", () => {
    const account = toRobloxAccount({ userId: "1234", username: "player", displayName: "Player" });
    expect(account.lastLoginAtSec).toBeNull();
  });
});
