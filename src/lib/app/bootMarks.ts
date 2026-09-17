/**
 * Boot milestones, in milliseconds since the page's time origin.
 *
 * The individual `queueLog` records cannot answer "where did the boot time
 * go": they are serialized through one IPC chain and stamped by the backend
 * when the write lands, so each one carries the queue's lag rather than the
 * moment it happened. These marks are taken in the webview with
 * `performance.now()` and shipped once, with `finish_boot`.
 */
const marks: Record<string, number> = {};

/**
 * The document's own marks, taken by the inline scripts in index.html before
 * any module ran. Without them the profile starts at `mainTs` and the whole
 * document phase, protocol request included, is one opaque number.
 */
function seedFromDocument(): void {
  const seed = (globalThis as { __accshiftBoot?: Record<string, unknown> }).__accshiftBoot;
  if (!seed) return;
  for (const [name, at] of Object.entries(seed)) {
    if (typeof at === "number" && marks[name] === undefined) {
      marks[name] = Math.round(at * 10) / 10;
    }
  }
}
seedFromDocument();

export function markBoot(name: string): void {
  if (typeof performance === "undefined") return;
  // First write wins: a milestone that somehow runs twice keeps its real one.
  if (marks[name] === undefined) marks[name] = Math.round(performance.now() * 10) / 10;
}

/** Milliseconds since `name` was marked, or null if it never was. */
export function sinceBoot(name: string): number | null {
  const at = marks[name];
  if (at === undefined || typeof performance === "undefined") return null;
  return Math.round((performance.now() - at) * 10) / 10;
}

export function bootMarks(): Record<string, number> {
  return { ...marks };
}
