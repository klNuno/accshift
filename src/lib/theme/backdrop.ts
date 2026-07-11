import { getCurrentWindow, Effect } from "@tauri-apps/api/window";

let appliedKey: string | null = null;

/**
 * Applies (or clears) the OS backdrop effect that blurs whatever sits behind
 * the transparent window. Driven entirely by the theme: glass themes get the
 * acrylic backdrop, everything else stays plain transparent. Windows only;
 * other platforms silently no-op (the try/catch also swallows unsupported-OS
 * errors from older Windows builds).
 */
export async function applyWindowBackdrop(glass: boolean): Promise<void> {
  const key = glass ? "acrylic" : "off";
  if (key === appliedKey) return;
  try {
    const appWindow = getCurrentWindow();
    if (glass) {
      await appWindow.setEffects({ effects: [Effect.Acrylic] });
    } else {
      await appWindow.clearEffects();
    }
    appliedKey = key;
  } catch {
    // Unsupported platform or webview: keep the plain transparent window.
  }
}
