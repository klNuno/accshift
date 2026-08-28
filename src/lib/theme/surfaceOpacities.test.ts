import { describe, expect, it } from "vitest";
import { getThemeDefinition, resolveThemeSurfaceOpacities } from "./themes";

const IDS = ["dark", "light", "midnight", "glass-dark", "glass-light", "liquid-glass"];
const PERCENTS = [0, 25, 42, 55, 80, 100];

/**
 * Every built-in theme, six slider positions, backdrop present and absent.
 *
 * These are the exact numbers the app painted with, captured before the four
 * surface rules became a table. A cap, a bump or a floor edited by accident
 * shows up here as a changed line instead of as a slightly wrong shade nobody
 * notices. Regenerate from a run, never by hand.
 */
const GOLDEN = [
  'dark 0 true {"windowOpacity":0,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'dark 0 false {"windowOpacity":0,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'dark 25 true {"windowOpacity":0.25,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'dark 25 false {"windowOpacity":0.25,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'dark 42 true {"windowOpacity":0.42,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'dark 42 false {"windowOpacity":0.42,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'dark 55 true {"windowOpacity":0.55,"cardOpacity":0.6900000000000001,"hoverOpacity":0.75,"mutedOpacity":0.73,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'dark 55 false {"windowOpacity":0.55,"cardOpacity":0.6900000000000001,"hoverOpacity":0.75,"mutedOpacity":0.73,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'dark 80 true {"windowOpacity":0.8,"cardOpacity":0.9400000000000001,"hoverOpacity":1,"mutedOpacity":0.98,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'dark 80 false {"windowOpacity":0.8,"cardOpacity":0.9400000000000001,"hoverOpacity":1,"mutedOpacity":0.98,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'dark 100 true {"windowOpacity":1,"cardOpacity":1,"hoverOpacity":1,"mutedOpacity":1,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'dark 100 false {"windowOpacity":1,"cardOpacity":1,"hoverOpacity":1,"mutedOpacity":1,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'light 0 true {"windowOpacity":0,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'light 0 false {"windowOpacity":0,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'light 25 true {"windowOpacity":0.25,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'light 25 false {"windowOpacity":0.25,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'light 42 true {"windowOpacity":0.42,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'light 42 false {"windowOpacity":0.42,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'light 55 true {"windowOpacity":0.55,"cardOpacity":0.6900000000000001,"hoverOpacity":0.75,"mutedOpacity":0.73,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'light 55 false {"windowOpacity":0.55,"cardOpacity":0.6900000000000001,"hoverOpacity":0.75,"mutedOpacity":0.73,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'light 80 true {"windowOpacity":0.8,"cardOpacity":0.9400000000000001,"hoverOpacity":1,"mutedOpacity":0.98,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'light 80 false {"windowOpacity":0.8,"cardOpacity":0.9400000000000001,"hoverOpacity":1,"mutedOpacity":0.98,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'light 100 true {"windowOpacity":1,"cardOpacity":1,"hoverOpacity":1,"mutedOpacity":1,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'light 100 false {"windowOpacity":1,"cardOpacity":1,"hoverOpacity":1,"mutedOpacity":1,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'midnight 0 true {"windowOpacity":0,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'midnight 0 false {"windowOpacity":0,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'midnight 25 true {"windowOpacity":0.25,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'midnight 25 false {"windowOpacity":0.25,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'midnight 42 true {"windowOpacity":0.42,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'midnight 42 false {"windowOpacity":0.42,"cardOpacity":0.66,"hoverOpacity":0.72,"mutedOpacity":0.72,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'midnight 55 true {"windowOpacity":0.55,"cardOpacity":0.6900000000000001,"hoverOpacity":0.75,"mutedOpacity":0.73,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'midnight 55 false {"windowOpacity":0.55,"cardOpacity":0.6900000000000001,"hoverOpacity":0.75,"mutedOpacity":0.73,"elevatedOpacity":0.78,"overlayOpacity":0.86,"isLiquid":false}',
  'midnight 80 true {"windowOpacity":0.8,"cardOpacity":0.9400000000000001,"hoverOpacity":1,"mutedOpacity":0.98,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'midnight 80 false {"windowOpacity":0.8,"cardOpacity":0.9400000000000001,"hoverOpacity":1,"mutedOpacity":0.98,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'midnight 100 true {"windowOpacity":1,"cardOpacity":1,"hoverOpacity":1,"mutedOpacity":1,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'midnight 100 false {"windowOpacity":1,"cardOpacity":1,"hoverOpacity":1,"mutedOpacity":1,"elevatedOpacity":1,"overlayOpacity":1,"isLiquid":false}',
  'glass-dark 0 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-dark 0 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-dark 25 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-dark 25 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-dark 42 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-dark 42 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-dark 55 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-dark 55 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-dark 80 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-dark 80 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-dark 100 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-dark 100 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-light 0 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-light 0 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-light 25 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-light 25 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-light 42 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-light 42 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-light 55 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-light 55 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-light 80 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-light 80 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'glass-light 100 true {"windowOpacity":0.55,"cardOpacity":0.65,"hoverOpacity":0.73,"mutedOpacity":0.6900000000000001,"elevatedOpacity":0.75,"overlayOpacity":0.86,"isLiquid":false}',
  'glass-light 100 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'liquid-glass 0 true {"windowOpacity":0.18,"cardOpacity":0.13,"hoverOpacity":0.21,"mutedOpacity":0.16,"elevatedOpacity":0.34,"overlayOpacity":0.86,"isLiquid":true}',
  'liquid-glass 0 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'liquid-glass 25 true {"windowOpacity":0.18,"cardOpacity":0.13,"hoverOpacity":0.21,"mutedOpacity":0.16,"elevatedOpacity":0.34,"overlayOpacity":0.86,"isLiquid":true}',
  'liquid-glass 25 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'liquid-glass 42 true {"windowOpacity":0.18,"cardOpacity":0.13,"hoverOpacity":0.21,"mutedOpacity":0.16,"elevatedOpacity":0.34,"overlayOpacity":0.86,"isLiquid":true}',
  'liquid-glass 42 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'liquid-glass 55 true {"windowOpacity":0.18,"cardOpacity":0.13,"hoverOpacity":0.21,"mutedOpacity":0.16,"elevatedOpacity":0.34,"overlayOpacity":0.86,"isLiquid":true}',
  'liquid-glass 55 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'liquid-glass 80 true {"windowOpacity":0.18,"cardOpacity":0.13,"hoverOpacity":0.21,"mutedOpacity":0.16,"elevatedOpacity":0.34,"overlayOpacity":0.86,"isLiquid":true}',
  'liquid-glass 80 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
  'liquid-glass 100 true {"windowOpacity":0.18,"cardOpacity":0.13,"hoverOpacity":0.21,"mutedOpacity":0.16,"elevatedOpacity":0.34,"overlayOpacity":0.86,"isLiquid":true}',
  'liquid-glass 100 false {"windowOpacity":0.96,"cardOpacity":0.72,"hoverOpacity":0.7999999999999999,"mutedOpacity":0.78,"elevatedOpacity":0.85,"overlayOpacity":1,"isLiquid":false}',
];

function measured(): string[] {
  const rows: string[] = [];
  for (const id of IDS) {
    const theme = getThemeDefinition(id);
    for (const percent of PERCENTS) {
      for (const backdropAvailable of [true, false]) {
        const values = resolveThemeSurfaceOpacities(theme, percent, { backdropAvailable });
        rows.push(`${id} ${percent} ${backdropAvailable} ${JSON.stringify(values)}`);
      }
    }
  }
  return rows;
}

describe("surface opacity rules", () => {
  it("paints every built-in theme exactly as before the table", () => {
    expect(measured()).toEqual(GOLDEN);
  });

  it("clamps a slider value outside 0 to 100", () => {
    const dark = getThemeDefinition("dark");
    expect(resolveThemeSurfaceOpacities(dark, -40).windowOpacity).toBe(0);
    expect(resolveThemeSurfaceOpacities(dark, 400).windowOpacity).toBe(1);
  });
});
