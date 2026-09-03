export type ToastType = "info" | "success" | "error";

export interface ToastAction {
  label: string;
  action: () => void;
}

export interface ToastMessage {
  id: string;
  message: string;
  durationMs: number | null;
  type: ToastType;
  toastAction?: ToastAction;
  resetKey: number;
}

export interface AddToastOptions {
  durationMs?: number | null;
  type?: ToastType;
  toastAction?: ToastAction;
}

const MAX_TOASTS = 5;
const DEFAULT_DURATION_MS = 3000;

/**
 * How long a toast stays, `null` meaning until the user dismisses it.
 *
 * An error never counts down. It is often the only trace of what failed, and
 * the old six second timer took it off screen while it was still being read.
 * An explicit duration does not override that; the close button and the hover
 * pause are the ways out. Every other type keeps its timer.
 */
export function resolveToastDuration(type: ToastType, requested?: number | null): number | null {
  if (type === "error") return null;
  return requested !== undefined ? requested : DEFAULT_DURATION_MS;
}

let toasts = $state<ToastMessage[]>([]);

export function getToasts() {
  return toasts;
}

export function addToast(message: string, options: AddToastOptions = {}): string {
  const type = options.type ?? "info";
  const durationMs = resolveToastDuration(type, options.durationMs);
  // Same message already on screen: restart its timer instead of stacking a duplicate.
  const existing = toasts.find((t) => t.message === message);
  if (existing) {
    existing.type = type;
    existing.durationMs = durationMs;
    existing.toastAction = options.toastAction ?? existing.toastAction;
    existing.resetKey += 1;
    return existing.id;
  }
  const id = crypto.randomUUID();
  toasts.push({
    id,
    message,
    durationMs,
    type,
    toastAction: options.toastAction,
    resetKey: 0,
  });
  if (toasts.length > MAX_TOASTS) {
    toasts.splice(0, toasts.length - MAX_TOASTS);
  }
  return id;
}

export function removeToast(id: string) {
  const idx = toasts.findIndex((t) => t.id === id);
  if (idx !== -1) {
    toasts.splice(idx, 1);
  }
}
