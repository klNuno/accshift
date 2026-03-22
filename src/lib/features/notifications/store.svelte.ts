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
}

export interface AddToastOptions {
  durationMs?: number | null;
  type?: ToastType;
  toastAction?: ToastAction;
}

let toasts = $state<ToastMessage[]>([]);

export function getToasts() {
  return toasts;
}

export function addToast(message: string, options: AddToastOptions = {}): string {
  const id = crypto.randomUUID();
  toasts.push({
    id,
    message,
    durationMs: options.durationMs ?? 3000,
    type: options.type ?? "info",
    toastAction: options.toastAction,
  });
  return id;
}

export function removeToast(id: string) {
  const idx = toasts.findIndex((t) => t.id === id);
  if (idx !== -1) {
    toasts.splice(idx, 1);
  }
}
