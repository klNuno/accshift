export interface ToastMessage {
  id: string;
  message: string;
}

let toasts = $state<ToastMessage[]>([]);

export function getToasts() {
  return toasts;
}

export function addToast(message: string) {
  const id = crypto.randomUUID();
  toasts.push({ id, message });
  // Auto-remove after 3 seconds
  setTimeout(() => {
    removeToast(id);
  }, 3000);
}

export function removeToast(id: string) {
  const idx = toasts.findIndex(t => t.id === id);
  if (idx !== -1) {
    toasts.splice(idx, 1);
  }
}

// Compatibility for existing calls, rerouted to toasts
export function addNotification(message: string) {
  addToast(message);
}

// Deprecated/No-op functions to avoid breaking imports immediately, 
// though we should clean them up.
export function getUnreadCount(): number {
  return 0;
}

export function clearNotifications() {
  // no-op
}
