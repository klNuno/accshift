import { describe, it, expect } from "vitest";
import { measureMaximizeButton, sameRect, type MeasuredRect } from "./snapLayouts";

function element(box: Partial<MeasuredRect>) {
  const full: MeasuredRect = { left: 0, top: 0, width: 46, height: 36, ...box };
  return { getBoundingClientRect: () => full };
}

describe("measureMaximizeButton", () => {
  it("turns a bounding box into the rect the backend expects", () => {
    expect(measureMaximizeButton(element({ left: 808, top: 0 }))).toEqual({
      x: 808,
      y: 0,
      width: 46,
      height: 36,
    });
  });

  it("reports nothing when the button is not rendered", () => {
    expect(measureMaximizeButton(null)).toBeNull();
    expect(measureMaximizeButton(undefined)).toBeNull();
  });

  it("reports nothing for a collapsed box", () => {
    expect(measureMaximizeButton(element({ width: 0 }))).toBeNull();
    expect(measureMaximizeButton(element({ height: 0 }))).toBeNull();
  });

  it("reports nothing when a coordinate is not a number", () => {
    expect(measureMaximizeButton(element({ left: Number.NaN }))).toBeNull();
    expect(measureMaximizeButton(element({ top: Number.POSITIVE_INFINITY }))).toBeNull();
  });

  it("rounds to two decimals so sub-pixel jitter stops at the IPC boundary", () => {
    expect(measureMaximizeButton(element({ left: 807.999999, width: 46.333333 }))).toEqual({
      x: 808,
      y: 0,
      width: 46.33,
      height: 36,
    });
  });
});

describe("sameRect", () => {
  const rect = { x: 808, y: 0, width: 46, height: 36 };

  it("matches equal rects and two absences", () => {
    expect(sameRect(rect, { ...rect })).toBe(true);
    expect(sameRect(null, null)).toBe(true);
  });

  it("separates a rect from an absence", () => {
    expect(sameRect(rect, null)).toBe(false);
    expect(sameRect(null, rect)).toBe(false);
  });

  it("catches a move of a hundredth of a pixel", () => {
    expect(sameRect(rect, { ...rect, x: 808.01 })).toBe(false);
    expect(sameRect(rect, { ...rect, height: 35 })).toBe(false);
  });
});
