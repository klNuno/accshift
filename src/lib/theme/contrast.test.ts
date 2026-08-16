import { describe, expect, it } from "vitest";
import { blend, contrastRatio, parseColor, relativeLuminance, roundRatio } from "./contrast";

describe("parseColor", () => {
  it("reads every notation a theme token may hold", () => {
    expect(parseColor("#fff")).toEqual({ r: 255, g: 255, b: 255 });
    expect(parseColor("#1c1c1f")).toEqual({ r: 28, g: 28, b: 31 });
    expect(parseColor("rgb(9, 9, 11)")).toEqual({ r: 9, g: 9, b: 11 });
    expect(parseColor("rgba(9 9 11 / 0.5)")).toEqual({ r: 9, g: 9, b: 11 });
    // The window fill is stored as a bare triplet so an alpha can be applied.
    expect(parseColor("9 9 11")).toEqual({ r: 9, g: 9, b: 11 });
    expect(parseColor("white")).toEqual({ r: 255, g: 255, b: 255 });
  });

  it("returns null rather than guessing", () => {
    expect(parseColor("")).toBeNull();
    expect(parseColor("#12345")).toBeNull();
    expect(parseColor("rebeccapurple")).toBeNull();
    expect(parseColor("var(--fg)")).toBeNull();
  });
});

describe("contrastRatio", () => {
  it("matches the WCAG anchors", () => {
    const white = { r: 255, g: 255, b: 255 };
    const black = { r: 0, g: 0, b: 0 };

    expect(relativeLuminance(white)).toBeCloseTo(1, 5);
    expect(relativeLuminance(black)).toBeCloseTo(0, 5);
    expect(roundRatio(contrastRatio(white, black))).toBe(21);
    expect(contrastRatio(white, white)).toBe(1);
    // Symmetric: the check does not care which colour is the background.
    expect(contrastRatio(black, white)).toBe(contrastRatio(white, black));
  });

  it("scores a mid grey on white the way WebAIM does", () => {
    expect(roundRatio(contrastRatio({ r: 119, g: 119, b: 119 }, { r: 255, g: 255, b: 255 }))).toBe(
      4.5,
    );
  });
});

describe("blend", () => {
  it("composites a translucent surface over its backdrop", () => {
    const white = { r: 255, g: 255, b: 255 };
    const dark = { r: 20, g: 20, b: 20 };

    expect(blend(white, 0, dark)).toEqual(dark);
    expect(blend(white, 1, dark)).toEqual(white);
    expect(blend(white, 0.5, dark)).toEqual({ r: 137.5, g: 137.5, b: 137.5 });
  });
});
