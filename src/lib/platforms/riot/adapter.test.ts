import { describe, it, expect } from "vitest";
import { toRiotAccount } from "./adapter";

describe("riot account mapping", () => {
  it("converts the backend millisecond stamp to seconds", () => {
    const account = toRiotAccount({
      id: "riot-1",
      label: "main",
      snapshot_state: "ready",
      last_used_at: 1_760_000_000_999,
    });
    expect(account.lastLoginAtSec).toBe(1_760_000_000);
  });

  it("falls back to the capture stamp, also in milliseconds", () => {
    const account = toRiotAccount({
      id: "riot-1",
      label: "main",
      snapshot_state: "ready",
      last_captured_at: 1_759_000_000_500,
    });
    expect(account.lastLoginAtSec).toBe(1_759_000_000);
  });

  it("keeps a missing stamp as null", () => {
    const account = toRiotAccount({ id: "riot-1", label: "main", snapshot_state: "ready" });
    expect(account.lastLoginAtSec).toBeNull();
  });
});
