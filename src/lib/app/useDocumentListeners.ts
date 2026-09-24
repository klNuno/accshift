import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { KeyScope } from "$lib/shared/keyboard/types";
import { CARD_NAV_SCOPES } from "./keyboardBindings";

type DocumentListenerDeps = {
  grid: { handleResize: (event: UIEvent) => void };
  drag: {
    handleDocMouseMove: (event: MouseEvent) => void;
    handleDocScroll: (event: Event) => void;
    handleDocMouseUp: (event: MouseEvent) => void;
    handleCaptureClick: (event: MouseEvent) => void;
  };
  bulkEdit: {
    handlePaintMouseMove: (event: MouseEvent) => void;
    handlePaintMouseUp: (event: MouseEvent) => void;
    handlePaintCaptureClick: (event: MouseEvent) => void;
  };
  uiScale: { handleCtrlWheelZoom: (event: WheelEvent) => void };
  keyboard: { attach: () => () => void };
  cardFocus: {
    clear: () => void;
    releaseIfOutside: (target: EventTarget | null) => void;
  };
  getKeyScope: () => KeyScope;
  appNavigation: { handlePopState: (event: PopStateEvent) => void };
  lifecycle: {
    handleWindowFocus: (event: FocusEvent) => void;
    handleVisibilityChange: (event: Event) => void;
  };
};

/**
 * The window and document listeners the app shell holds for its whole life:
 * grid resize, card drag, bulk edit paint selection, ctrl+wheel zoom, the
 * keyboard dispatcher, card focus release, history and window focus. `attach`
 * and `detach` keep the order they always had.
 */
export function createDocumentListeners(deps: DocumentListenerDeps) {
  let detachKeyboard: (() => void) | null = null;

  // Real focus or a pointer press elsewhere ends keyboard roving on the grid,
  // so Enter and Space go to the control the user is on. Overlays (menus,
  // dialogs, palette) are left alone: closing them returns to the same card.
  function releaseCardFocusOnFocusIn(event: FocusEvent) {
    if (!CARD_NAV_SCOPES.includes(deps.getKeyScope())) return;
    deps.cardFocus.releaseIfOutside(event.target);
  }

  function releaseCardFocusOnPointerDown() {
    if (!CARD_NAV_SCOPES.includes(deps.getKeyScope())) return;
    deps.cardFocus.clear();
  }

  function attach() {
    const { grid, drag, bulkEdit, uiScale, keyboard, appNavigation, lifecycle } = deps;
    window.addEventListener("resize", grid.handleResize);
    document.addEventListener("mousemove", drag.handleDocMouseMove);
    document.addEventListener("scroll", drag.handleDocScroll, true);
    document.addEventListener("mouseup", drag.handleDocMouseUp);
    document.addEventListener("click", drag.handleCaptureClick, true);
    document.addEventListener("mousemove", bulkEdit.handlePaintMouseMove);
    document.addEventListener("mouseup", bulkEdit.handlePaintMouseUp);
    document.addEventListener("click", bulkEdit.handlePaintCaptureClick, true);
    window.addEventListener("wheel", uiScale.handleCtrlWheelZoom, { passive: false });
    detachKeyboard = keyboard.attach();
    document.addEventListener("focusin", releaseCardFocusOnFocusIn);
    document.addEventListener("pointerdown", releaseCardFocusOnPointerDown, true);
    window.addEventListener("popstate", appNavigation.handlePopState);
    window.addEventListener("focus", lifecycle.handleWindowFocus);
    document.addEventListener("visibilitychange", lifecycle.handleVisibilityChange);
  }

  function detach() {
    const { grid, drag, bulkEdit, uiScale, appNavigation, lifecycle } = deps;
    window.removeEventListener("resize", grid.handleResize);
    document.removeEventListener("mousemove", drag.handleDocMouseMove);
    document.removeEventListener("scroll", drag.handleDocScroll, true);
    document.removeEventListener("mouseup", drag.handleDocMouseUp);
    document.removeEventListener("click", drag.handleCaptureClick, true);
    document.removeEventListener("mousemove", bulkEdit.handlePaintMouseMove);
    document.removeEventListener("mouseup", bulkEdit.handlePaintMouseUp);
    document.removeEventListener("click", bulkEdit.handlePaintCaptureClick, true);
    window.removeEventListener("wheel", uiScale.handleCtrlWheelZoom);
    detachKeyboard?.();
    detachKeyboard = null;
    document.removeEventListener("focusin", releaseCardFocusOnFocusIn);
    document.removeEventListener("pointerdown", releaseCardFocusOnPointerDown, true);
    window.removeEventListener("popstate", appNavigation.handlePopState);
    window.removeEventListener("focus", lifecycle.handleWindowFocus);
    document.removeEventListener("visibilitychange", lifecycle.handleVisibilityChange);
  }

  return { attach, detach };
}

type CloseRequestDeps = {
  /** Flushes pending storage and settings saves before the window goes. */
  flush: () => Promise<void>;
};

/**
 * Flush pending storage saves on close, then destroy the window. The flag
 * stops destroy() from re-entering our own preventDefault into a loop.
 */
export function createCloseRequestHandler({ flush }: CloseRequestDeps) {
  let unlistenCloseRequested: UnlistenFn | null = null;
  let closeHandlerDisposed = false;
  let isClosing = false;

  function register() {
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        if (isClosing) return;
        isClosing = true;
        event.preventDefault();
        try {
          await flush();
        } catch (e) {
          console.error("Failed to flush pending saves on close:", e);
        }
        await getCurrentWindow().destroy();
      })
      .then((unlisten) => {
        // The component may have been destroyed before the listener resolved.
        if (closeHandlerDisposed) {
          unlisten();
        } else {
          unlistenCloseRequested = unlisten;
        }
      })
      .catch((e) => {
        console.error("Failed to register close handler:", e);
      });
  }

  function dispose() {
    closeHandlerDisposed = true;
    unlistenCloseRequested?.();
    unlistenCloseRequested = null;
  }

  return { register, dispose };
}
