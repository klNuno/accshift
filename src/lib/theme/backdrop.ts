import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, Effect } from "@tauri-apps/api/window";

let appliedKey: string | null = null;

/** Acrylic tint per glass theme. Only honored on the SWCA acrylic path
 * (Windows 10 / early 11); DWM system backdrops ignore the color, and macOS
 * ignores it too. The Windows default tint is a mid gray that reads plastic:
 * glass dark wants a heavy black so the backdrop reads black instead of gray. */
const ACRYLIC_TINTS: Record<string, [number, number, number, number]> = {
  "glass-dark": [0, 0, 0, 150],
};

/** macOS vibrancy material per glass theme. setEffects applies the first
 * effect the platform supports, so listing a Windows effect and a macOS one
 * together covers both from a single call. */
const MAC_EFFECTS: Record<string, Effect> = {
  "glass-dark": Effect.HudWindow,
  "glass-light": Effect.Popover,
  "liquid-glass": Effect.UnderWindowBackground,
};

/**
 * Applies (or clears) the OS backdrop effect that blurs whatever sits behind
 * the transparent window. Driven entirely by the theme: glass themes get the
 * platform's blur material, everything else stays plain transparent. Linux
 * has no cross-compositor blur protocol: the call no-ops there and
 * applyThemeToDocument paints a near-solid window instead (osBackdrop: false).
 *
 * Windows materials: glass dark/light use Acrylic (the only DWM backdrop
 * compatible with transparent(true) — Mica/Tabbed render black on layered
 * windows). Liquid glass uses NO Windows material at all: the window stays
 * purely transparent and the crisp desktop shows through the CSS surfaces.
 * DWM stops rendering system backdrops on unfocused windows, so a native
 * WM_NCACTIVATE subclass (set_keep_backdrop_active) keeps them alive while an
 * acrylic glass theme is active.
 */
export async function applyWindowBackdrop(glass: boolean, themeId: string): Promise<void> {
  const key = glass ? `backdrop:${themeId}` : "off";
  if (key === appliedKey) return;
  try {
    const appWindow = getCurrentWindow();
    const acrylic = glass && themeId !== "liquid-glass";
    if (glass) {
      if (!acrylic) {
        // apply_effects with no Windows effect in the list is a no-op there,
        // so drop any acrylic left by a previous glass theme explicitly.
        await appWindow.clearEffects();
      }
      const tint = ACRYLIC_TINTS[themeId];
      await appWindow.setEffects({
        effects: [
          ...(acrylic ? [Effect.Acrylic] : []),
          MAC_EFFECTS[themeId] ?? Effect.UnderWindowBackground,
        ],
        ...(tint ? { color: tint } : {}),
      });
    } else {
      await appWindow.clearEffects();
    }
    void invoke("set_keep_backdrop_active", { enabled: acrylic }).catch(() => {});
    appliedKey = key;
  } catch {
    // Unsupported platform or webview: keep the plain transparent window.
  }
}
