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
const ERROR_DURATION_MS = 6000;

let toasts = $state<ToastMessage[]>([]);

export function getToasts() {
  return toasts;
}

export function addToast(message: string, options: AddToastOptions = {}): string {
  const type = options.type ?? "info";
  const durationMs =
    options.durationMs !== undefined
      ? options.durationMs
      : type === "error"
        ? ERROR_DURATION_MS
        : DEFAULT_DURATION_MS;
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
