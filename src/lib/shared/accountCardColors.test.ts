import { describe, it, expect, vi } from "vitest";

vi.mock("$lib/storage/clientStorage", () => {
  let stores = new Map<string, unknown>();
  let revisions = new Map<string, number>();
  return {
    CLIENT_STORE_ACCOUNT_CARD_COLORS: "client.account-card-colors",
    getClientStoreValue: (id: string) => stores.get(id) ?? null,
    setClientStoreValue: (id: string, value: unknown) => {
      stores.set(id, value);
      revisions.set(id, (revisions.get(id) ?? 0) + 1);
    },
    getClientStoreRevision: (id: string) => revisions.get(id) ?? 0,
  };
});

import {
  getAccountCardColor,
  setAccountCardColor,
  ACCOUNT_CARD_COLOR_PRESETS,
} from "./accountCardColors";

describe("accountCardColors", () => {
  it("getAccountCardColor returns empty string for unknown account", () => {
    expect(getAccountCardColor("unknown-id")).toBe("");
  });

  it("setAccountCardColor stores valid hex colors", () => {
    setAccountCardColor("acc1", "#ff0000");
    expect(getAccountCardColor("acc1")).toBe("#ff0000");

    setAccountCardColor("acc2", "#abc");
    expect(getAccountCardColor("acc2")).toBe("#abc");

    setAccountCardColor("acc3", "#aabbccdd");
    expect(getAccountCardColor("acc3")).toBe("#aabbccdd");
  });

  it("setAccountCardColor rejects invalid colors (non-hex)", () => {
    setAccountCardColor("acc1", "red");
    expect(getAccountCardColor("acc1")).toBe("");

    setAccountCardColor("acc2", "rgb(255,0,0)");
    expect(getAccountCardColor("acc2")).toBe("");

    setAccountCardColor("acc3", "#xyz");
    expect(getAccountCardColor("acc3")).toBe("");

    setAccountCardColor("acc4", "not-a-color");
    expect(getAccountCardColor("acc4")).toBe("");
  });

  it("ACCOUNT_CARD_COLOR_PRESETS has expected structure", () => {
    expect(ACCOUNT_CARD_COLOR_PRESETS.length).toBeGreaterThan(0);

    for (const preset of ACCOUNT_CARD_COLOR_PRESETS) {
      expect(preset).toHaveProperty("id");
      expect(preset).toHaveProperty("color");
      expect(typeof preset.id).toBe("string");
      expect(typeof preset.color).toBe("string");
    }

    const none = ACCOUNT_CARD_COLOR_PRESETS.find((p) => p.id === "none");
    expect(none).toBeDefined();
    expect(none!.color).toBe("");
  });
});
