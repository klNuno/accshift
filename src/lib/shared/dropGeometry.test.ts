import { describe, expect, it } from "vitest";
import { previewIndexAt, type SlotRect } from "./dropGeometry";

/** Four 100x100 cards in a row, gapless, starting at the origin. */
const ROW: SlotRect[] = [0, 1, 2, 3].map((i) => ({
  left: i * 100,
  top: 0,
  width: 100,
  height: 100,
}));

/** The same four stacked, which is what list mode lays out. */
const COLUMN: SlotRect[] = [0, 1, 2, 3].map((i) => ({
  left: 0,
  top: i * 100,
  width: 100,
  height: 100,
}));

describe("previewIndexAt", () => {
  it("has nowhere to drop when the layout was never measured", () => {
    expect(previewIndexAt(50, 50, [], 0, false)).toBeNull();
    expect(previewIndexAt(50, 50, ROW, -1, false)).toBeNull();
  });

  it("drops before the nearest card when the pointer is on its leading half", () => {
    expect(previewIndexAt(210, 50, ROW, 0, false)).toBe(1);
  });

  it("drops after the nearest card when the pointer is past its middle", () => {
    expect(previewIndexAt(260, 50, ROW, 0, false)).toBe(2);
  });

  it("reads the vertical axis in list mode and the horizontal one in a grid", () => {
    // Same point, same slots: only the axis that decides before-or-after moves.
    expect(previewIndexAt(60, 240, COLUMN, 3, true)).toBe(2);
    expect(previewIndexAt(60, 260, COLUMN, 3, true)).toBe(3);
  });

  it("closes the gap the card left behind when it moves forward", () => {
    // Card 0 dropped past card 2: without the shift it would land at 3 and sit
    // one slot too far, because removing it first pulls everything left.
    expect(previewIndexAt(260, 50, ROW, 0, false)).toBe(2);
    // Moving backwards leaves the tail untouched, so no shift applies.
    expect(previewIndexAt(30, 50, ROW, 3, false)).toBe(0);
  });

  it("never points outside the list", () => {
    expect(previewIndexAt(-5000, 50, ROW, 2, false)).toBe(0);
    expect(previewIndexAt(5000, 50, ROW, 0, false)).toBe(3);
  });

  it("resolves a pointer that is inside no card at all", () => {
    // Gutters: cards occupy 0-100, 120-220, 240-340, 360-460.
    const gapped: SlotRect[] = [0, 1, 2, 3].map((i) => ({
      left: i * 120,
      top: 0,
      width: 100,
      height: 100,
    }));
    // 115 is between card 0 and card 1, nearer to card 1's centre.
    expect(previewIndexAt(115, 50, gapped, 3, false)).toBe(1);
  });
});
