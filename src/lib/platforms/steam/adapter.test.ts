import { describe, it, expect } from "vitest";
import { toSteamAccount } from "./adapter";

// Steam is the exception: loginusers.vdf already holds Unix seconds, so the
// mapping passes the value through untouched.
describe("steam account mapping", () => {
  it("passes the seconds stamp through unchanged", () => {
    const account = toSteamAccount({
      steam_id: "76561198000000001",
      account_name: "player",
      persona_name: "Player",
      last_login_at: 1_760_000_000,
    });
    expect(account.lastLoginAtSec).toBe(1_760_000_000);
  });

  it("keeps a missing stamp as null", () => {
    const account = toSteamAccount({
      steam_id: "76561198000000001",
      account_name: "player",
      persona_name: "Player",
    });
    expect(account.lastLoginAtSec).toBeNull();
  });
});
