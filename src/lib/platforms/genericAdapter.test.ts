import { describe, it, expect } from "vitest";
import { defaultToAccount } from "./genericAdapter";

// The descriptor platforms (GOG, Jagex, Epic, Discord, Ubisoft) all stamp
// `platforms::now_unix_ms` on the Rust side, so the mapping owes the UI seconds.
describe("generic adapter account mapping", () => {
  it("converts the backend millisecond stamp to seconds", () => {
    const account = defaultToAccount({
      accountId: "acct-1",
      label: "main",
      lastUsedAt: 1_760_000_000_999,
    });
    expect(account.lastLoginAtSec).toBe(1_760_000_000);
  });

  it("keeps a missing stamp as null", () => {
    expect(defaultToAccount({ accountId: "acct-1", label: "main" }).lastLoginAtSec).toBeNull();
    expect(
      defaultToAccount({ accountId: "acct-1", label: "main", lastUsedAt: null }).lastLoginAtSec,
    ).toBeNull();
  });

  it("falls back to the account id when the label is empty", () => {
    const account = defaultToAccount({ accountId: "acct-1", label: "" });
    expect(account.displayName).toBe("acct-1");
  });
});
