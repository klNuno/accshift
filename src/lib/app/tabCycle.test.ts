import { describe, expect, it } from "vitest";
import { pickCycledTab } from "./tabCycle";

describe("pickCycledTab", () => {
  const usable = ["steam", "riot", "epic"];

  it("steps forward and backward with wrap-around", () => {
    expect(pickCycledTab(usable, "steam", 1)).toBe("riot");
    expect(pickCycledTab(usable, "epic", 1)).toBe("steam");
    expect(pickCycledTab(usable, "riot", -1)).toBe("steam");
    expect(pickCycledTab(usable, "steam", -1)).toBe("epic");
  });

  it("enters from the matching end when the active tab is not usable", () => {
    expect(pickCycledTab(usable, "roblox", 1)).toBe("steam");
    expect(pickCycledTab(usable, "roblox", -1)).toBe("epic");
  });

  it("returns null with fewer than two usable tabs", () => {
    expect(pickCycledTab(["steam"], "steam", 1)).toBeNull();
    expect(pickCycledTab([], "steam", -1)).toBeNull();
  });
});
