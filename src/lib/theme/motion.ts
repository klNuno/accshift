import type { AnimationsMode } from "$lib/features/settings/types";

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

/**
 * Applies the animations preference as `data-motion="full" | "reduced"` on
 * <html>. Stylesheets target `html[data-motion="reduced"]` instead of the
 * `prefers-reduced-motion` media query so the user can override the OS.
 * Returns a cleanup that detaches the OS listener (attached in "system" mode).
 */
export function applyMotionPreference(mode: AnimationsMode, doc: Document = document): () => void {
  const query = doc.defaultView?.matchMedia(REDUCED_MOTION_QUERY) ?? null;
  const apply = () => {
    const reduced = mode === "off" || (mode === "system" && (query?.matches ?? false));
    doc.documentElement.dataset.motion = reduced ? "reduced" : "full";
  };
  apply();
  if (mode === "system" && query) {
    query.addEventListener("change", apply);
    return () => query.removeEventListener("change", apply);
  }
  return () => {};
}
