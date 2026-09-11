/**
 * Geometry the backend needs to answer HTMAXBUTTON over our own maximize
 * button, which is what makes Windows 11 offer the Snap Layouts flyout on a
 * frameless window. The button is drawn by TitleBar.svelte, so the frontend is
 * the only side that knows where it is.
 */

/** The button in CSS pixels, relative to the top-left of the webview. */
export type MaximizeButtonRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

/** What `getBoundingClientRect()` gives back, narrowed to what is used here. */
export type MeasuredRect = {
  left: number;
  top: number;
  width: number;
  height: number;
};

/** Anything that can be measured. An `HTMLElement` satisfies this. */
export type Measurable = { getBoundingClientRect(): MeasuredRect };

/** Two decimals: enough for a 46px button on a 300% display, and it keeps a
    sub-pixel jitter from firing an IPC call on every animation frame. */
function round(value: number): number {
  return Math.round(value * 100) / 100;
}

/**
 * The button's rectangle, or `null` when there is nothing to report: no
 * element (macOS layout, actions hidden), a collapsed box (`display: none`),
 * or a browser that hands back something non-finite.
 */
export function measureMaximizeButton(
  element: Measurable | null | undefined,
): MaximizeButtonRect | null {
  if (!element) return null;
  const box = element.getBoundingClientRect();
  const values = [box.left, box.top, box.width, box.height];
  if (!values.every((value) => Number.isFinite(value))) return null;
  if (box.width <= 0 || box.height <= 0) return null;
  return {
    x: round(box.left),
    y: round(box.top),
    width: round(box.width),
    height: round(box.height),
  };
}

/** True when the two rectangles say the same thing, `null` included. Used to
    skip an IPC call that would tell the backend what it already knows. */
export function sameRect(a: MaximizeButtonRect | null, b: MaximizeButtonRect | null): boolean {
  if (a === null || b === null) return a === b;
  return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
}
