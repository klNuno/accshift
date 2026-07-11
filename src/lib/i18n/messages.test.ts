import { describe, it, expect } from "vitest";
import { EN_MESSAGES } from "./messages";
import { FR_MESSAGES } from "./messages.fr";

describe("message dictionaries", () => {
  it("fr covers exactly the en key set", () => {
    const enKeys = Object.keys(EN_MESSAGES).sort();
    const frKeys = Object.keys(FR_MESSAGES).sort();
    expect(frKeys).toEqual(enKeys);
  });

  it("contains no em or en dashes in any UI string", () => {
    for (const [key, value] of [...Object.entries(EN_MESSAGES), ...Object.entries(FR_MESSAGES)]) {
      expect(value, `dash in ${key}`).not.toMatch(/[–—]/);
    }
  });

  it("has no empty strings", () => {
    for (const [key, value] of [...Object.entries(EN_MESSAGES), ...Object.entries(FR_MESSAGES)]) {
      expect(value.trim(), `empty message for ${key}`).not.toBe("");
    }
  });
});
