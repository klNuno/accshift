import type { PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { isPinLockedError } from "$lib/shared/pinSession";

type ContextMenuErrorFormatter = (
  error: unknown,
  callbacks: PlatformContextMenuCallbacks,
) => string;

function defaultErrorFormatter(error: unknown): string {
  return String(error);
}

export function createSafeContextAction(
  callbacks: PlatformContextMenuCallbacks,
  task: () => void | Promise<void>,
  formatError: ContextMenuErrorFormatter = defaultErrorFormatter,
): () => Promise<void> {
  return async () => {
    try {
      await task();
    } catch (error) {
      // A PIN refusal already brought the lock screen up: no toast on top.
      if (isPinLockedError(error)) return;
      callbacks.showToast(formatError(error, callbacks));
    }
  };
}

export function confirmSafeContextAction(
  callbacks: PlatformContextMenuCallbacks,
  config: {
    title: string;
    message: string;
    confirmLabel?: string;
  },
  task: () => void | Promise<void>,
  formatError?: ContextMenuErrorFormatter,
) {
  callbacks.confirmAction({
    ...config,
    onConfirm: createSafeContextAction(callbacks, task, formatError),
  });
}
