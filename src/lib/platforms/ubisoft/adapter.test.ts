import { describe, it, expect } from "vitest";
import { toUbisoftAccount } from "./adapter";

describe("ubisoft account mapping", () => {
  it("converts the backend millisecond stamp to seconds", () => {
    const account = toUbisoftAccount({
      uuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
      label: "main",
      lastUsedAt: 1_760_000_000_999,
      snapshotSaved: true,
    });
    expect(account.lastLoginAtSec).toBe(1_760_000_000);
  });

  it("keeps a missing stamp as null", () => {
    const account = toUbisoftAccount({
      uuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
      label: "main",
      snapshotSaved: false,
    });
    expect(account.lastLoginAtSec).toBeNull();
  });

  it("shortens the uuid when there is no label", () => {
    const account = toUbisoftAccount({
      uuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
      label: "",
      snapshotSaved: false,
    });
    expect(account.displayName).toBe("aaaaaaaa");
  });
});
