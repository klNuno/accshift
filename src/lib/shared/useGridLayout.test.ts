import { describe, it, expect } from "vitest";
import { CARD_WIDTH, cardWidthForDensity } from "./useGridLayout.svelte";

describe("cardWidthForDensity", () => {
  it("is the base width at cozy scale", () => {
    expect(cardWidthForDensity(1)).toBe(CARD_WIDTH);
  });

  it("shrinks cards on compact", () => {
    expect(cardWidthForDensity(0.88)).toBeCloseTo(88, 6);
  });

  it("grows cards on comfortable", () => {
    expect(cardWidthForDensity(1.12)).toBeCloseTo(112, 6);
  });

  it("falls back to the base width on garbage input", () => {
    expect(cardWidthForDensity(Number.NaN)).toBe(CARD_WIDTH);
    expect(cardWidthForDensity(0)).toBe(CARD_WIDTH);
    expect(cardWidthForDensity(-2)).toBe(CARD_WIDTH);
  });
});
