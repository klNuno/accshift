import { getCurrentWindow, Effect } from "@tauri-apps/api/window";

let appliedKey: string | null = null;

/**
 * Applies (or clears) the OS backdrop effect that blurs whatever sits behind
 * the transparent window. Windows only: 1-49% maps to the lighter legacy blur,
 * 50-100% to acrylic. Other platforms silently no-op (the try/catch also
 * swallows unsupported-OS errors from older Windows builds).
 */
export async function applyWindowBackdrop(blurPercent: number): Promise<void> {
  const clamped = Math.min(100, Math.max(0, Math.round(blurPercent)));
  const key = clamped === 0 ? "off" : clamped < 50 ? "blur" : "acrylic";
  if (key === appliedKey) return;
  try {
    const appWindow = getCurrentWindow();
    if (key === "off") {
      await appWindow.clearEffects();
    } else {
      await appWindow.setEffects({ effects: [key === "blur" ? Effect.Blur : Effect.Acrylic] });
    }
    appliedKey = key;
  } catch {
    // Unsupported platform or webview: keep the plain transparent window.
  }
}
