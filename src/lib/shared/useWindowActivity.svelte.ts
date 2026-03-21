import { getCurrentWindow } from "@tauri-apps/api/window";

export function createWindowActivity() {
  const appWindow = typeof window !== "undefined" ? getCurrentWindow() : null;
  let isFocused = $state(true);
  let isMinimized = $state(false);
  let isPageVisible = $state(true);
  let started = false;
  let syncing = false;
  let cleanupFns: Array<() => void | Promise<void>> = [];

  function updatePageVisibility() {
    if (typeof document === "undefined") return;
    isPageVisible = document.visibilityState !== "hidden";
  }

  async function sync() {
    updatePageVisibility();
    if (!appWindow || syncing) return;
    syncing = true;
    try {
      const [focusedResult, minimizedResult] = await Promise.allSettled([
        appWindow.isFocused(),
        appWindow.isMinimized(),
      ]);

      if (focusedResult.status === "fulfilled") {
        isFocused = focusedResult.value;
      }
      if (minimizedResult.status === "fulfilled") {
        isMinimized = Boolean(minimizedResult.value);
      }
    } finally {
      syncing = false;
    }
  }

  async function start() {
    if (started) return;
    started = true;
    updatePageVisibility();

    if (typeof window !== "undefined") {
      const handleWindowFocus = () => {
        isFocused = true;
        void sync();
      };
      const handleWindowBlur = () => {
        isFocused = false;
        void sync();
      };
      window.addEventListener("focus", handleWindowFocus);
      window.addEventListener("blur", handleWindowBlur);
      cleanupFns.push(() => window.removeEventListener("focus", handleWindowFocus));
      cleanupFns.push(() => window.removeEventListener("blur", handleWindowBlur));
    }

    if (typeof document !== "undefined") {
      const handleVisibilityChange = () => {
        updatePageVisibility();
        void sync();
      };
      document.addEventListener("visibilitychange", handleVisibilityChange);
      cleanupFns.push(() =>
        document.removeEventListener("visibilitychange", handleVisibilityChange),
      );
    }

    if (!appWindow) return;

    const [unlistenFocus, unlistenResize] = await Promise.all([
      appWindow.onFocusChanged(({ payload }) => {
        isFocused = payload;
        if (payload) {
          isMinimized = false;
        }
        updatePageVisibility();
      }),
      appWindow.onResized(() => {
        void sync();
      }),
    ]);

    cleanupFns.push(unlistenFocus);
    cleanupFns.push(unlistenResize);

    void sync();
  }

  function stop() {
    if (!started) return;
    started = false;
    for (const cleanup of cleanupFns.splice(0)) {
      cleanup();
    }
  }

  return {
    get isFocused() {
      return isFocused;
    },
    get isMinimized() {
      return isMinimized;
    },
    get isPageVisible() {
      return isPageVisible;
    },
    get isForeground() {
      return isFocused && isPageVisible && !isMinimized;
    },
    start,
    stop,
    sync,
  };
}
