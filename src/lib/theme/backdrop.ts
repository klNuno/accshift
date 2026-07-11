import { getCurrentWindow, Effect } from "@tauri-apps/api/window";

let appliedKey: string | null = null;

/**
 * Applies (or clears) the OS backdrop effect that blurs whatever sits behind
 * the transparent window. Driven entirely by the theme: glass themes get the
 * acrylic backdrop, everything else stays plain transparent. Windows only;
 * other platforms silently no-op (the try/catch also swallows unsupported-OS
 * errors from older Windows builds).
 */
export async function applyWindowBackdrop(glass: boolean, liquid = false): Promise<void> {
  const key = glass ? (liquid ? "acrylic-liquid" : "acrylic") : "off";
  if (key === appliedKey) return;
  try {
    const appWindow = getCurrentWindow();
    if (glass) {
      // Liquid glass gets a near-clear white tint: the default acrylic tint
      // reads gray and plastic, this keeps the blurred desktop luminous.
      await appWindow.setEffects({
        effects: [Effect.Acrylic],
        ...(liquid ? { color: [255, 255, 255, 12] as [number, number, number, number] } : {}),
      });
    } else {
      await appWindow.clearEffects();
    }
    appliedKey = key;
  } catch {
    // Unsupported platform or webview: keep the plain transparent window.
  }
}
