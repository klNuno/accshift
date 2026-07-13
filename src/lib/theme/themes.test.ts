import { describe, expect, it } from "vitest";
import { getThemeDefinition, resolveThemeSurfaceOpacities } from "./themes";

describe("theme surface fallback", () => {
  it("uses tuned translucent values when the Liquid Glass wallpaper is available", () => {
    const values = resolveThemeSurfaceOpacities(getThemeDefinition("liquid-glass"), 50, {
      backdropAvailable: true,
    });

    expect(values.isLiquid).toBe(true);
    expect(values.windowOpacity).toBe(0.18);
    expect(values.cardOpacity).toBe(0.13);
  });

  it("uses a readable near-solid fallback when wallpaper capture fails", () => {
    const values = resolveThemeSurfaceOpacities(getThemeDefinition("liquid-glass"), 50, {
      backdropAvailable: false,
    });

    expect(values.isLiquid).toBe(false);
    expect(values.windowOpacity).toBe(0.96);
    expect(values.cardOpacity).toBe(0.72);
    expect(values.overlayOpacity).toBe(1);
  });

  it("does not change regular theme opacity when no backdrop exists", () => {
    const values = resolveThemeSurfaceOpacities(getThemeDefinition("dark"), 42, {
      backdropAvailable: false,
    });

    expect(values.windowOpacity).toBe(0.42);
  });
});
