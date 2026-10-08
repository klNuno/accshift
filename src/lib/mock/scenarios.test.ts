import { describe, expect, it } from "vitest";
import { SCENARIOS } from "./scenarios";

describe("demo scenario", () => {
  it("places every Steam account exactly once, on the grid or in a folder", () => {
    const spec = SCENARIOS.demo();
    const folders = spec.stores["client.folders"] as {
      itemOrder: Record<string, { type: string; id: string }[]>;
    };
    const placed = Object.values(folders.itemOrder)
      .flat()
      .filter((item) => item.type === "account")
      .map((item) => item.id);

    expect(placed).toHaveLength(new Set(placed).size);
    expect(new Set(placed)).toEqual(new Set(spec.steamAccounts.map((a) => a.steam_id)));
    expect(folders.itemOrder["demo-folder-smurfs"]).toHaveLength(4);
  });
});
