import { describe, it, expect, vi } from "vitest";

vi.mock("$lib/storage/clientStorage", () => {
  let stores = new Map<string, unknown>();
  let revisions = new Map<string, number>();
  return {
    CLIENT_STORE_ACCOUNT_CARD_NOTES: "client.account-card-notes",
    getClientStoreValue: (id: string) => stores.get(id) ?? null,
    setClientStoreValue: (id: string, value: unknown) => {
      stores.set(id, value);
      revisions.set(id, (revisions.get(id) ?? 0) + 1);
    },
    getClientStoreRevision: (id: string) => revisions.get(id) ?? 0,
  };
});

import { getAccountCardNote, setAccountCardNote, clearAccountCardNote } from "./accountCardNotes";

describe("accountCardNotes", () => {
  it("getAccountCardNote returns empty string for unknown", () => {
    expect(getAccountCardNote("unknown-id")).toBe("");
  });

  it("setAccountCardNote stores trimmed text", () => {
    setAccountCardNote("acc1", "  hello world  ");
    expect(getAccountCardNote("acc1")).toBe("hello world");
  });

  it("setAccountCardNote strips control characters", () => {
    setAccountCardNote("acc1", "line1\x00line2\x01line3");
    const result = getAccountCardNote("acc1");
    expect(result).not.toMatch(/\x00/);
    expect(result).not.toMatch(/\x01/);
    expect(result).toBe("line1 line2 line3");
  });

  it("setAccountCardNote truncates at 180 chars", () => {
    const longText = "a".repeat(300);
    setAccountCardNote("acc1", longText);
    expect(getAccountCardNote("acc1")).toHaveLength(180);
  });

  it("clearAccountCardNote removes the note", () => {
    setAccountCardNote("acc1", "some note");
    expect(getAccountCardNote("acc1")).toBe("some note");

    clearAccountCardNote("acc1");
    expect(getAccountCardNote("acc1")).toBe("");
  });
});
