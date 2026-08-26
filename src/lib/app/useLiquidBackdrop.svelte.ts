import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** The desktop wallpaper and where it sits on the virtual screen, physical px. */
export type WallpaperSnapshot = {
  dataUrl: string;
  x: number;
  y: number;
  width: number;
  height: number;
};

type LiquidBackdropDeps = {
  /** True while the Liquid Glass theme is up on a platform that can fake a backdrop. */
  isActive: () => boolean;
};

/** Extra wallpaper drawn past the window edge so a move never reveals a gap. */
const BLEED_PX = 40;
/** The user can change their wallpaper without the app hearing about it. */
const REFRESH_MS = 5 * 60 * 1000;
/** A resize fires continuously; only re-snapshot once it settles. */
const RESIZE_SETTLE_MS = 300;

/**
 * Fake backdrop for the Liquid Glass theme on Windows.
 *
 * DWM offers no material that blurs *and* refracts what sits behind a
 * transparent window, so the desktop wallpaper is replicated inside the shell,
 * aligned to the screen through `background-position`, and filtered in CSS
 * (see `.liquid-backdrop`). Moving the window re-aligns the layer, which is
 * what makes it read as true see-through glass rather than a printed texture.
 *
 * Everything degrades to a plain transparent window: a failed snapshot, a
 * missing window API, a platform with no wallpaper to read.
 */
export function createLiquidBackdrop({ isActive }: LiquidBackdropDeps) {
  let wallpaper = $state<WallpaperSnapshot | null>(null);
  let style = $state("");

  /**
   * Re-align the replicated wallpaper with the screen after the window moved.
   *
   * Snapshot rect and window position are both physical virtual-screen px, so
   * dividing by the window's scale factor gives the CSS px the layer needs.
   */
  async function realign() {
    const snapshot = wallpaper;
    if (!snapshot) return;
    try {
      const appWindow = getCurrentWindow();
      const [pos, scale] = await Promise.all([appWindow.outerPosition(), appWindow.scaleFactor()]);
      const offsetX = (snapshot.x - pos.x) / scale + BLEED_PX;
      const offsetY = (snapshot.y - pos.y) / scale + BLEED_PX;
      style =
        `background-size:${snapshot.width / scale}px ${snapshot.height / scale}px;` +
        `background-position:${offsetX}px ${offsetY}px;`;
    } catch {
      // Window APIs unavailable: keep the plain transparent look.
    }
  }

  $effect(() => {
    if (!isActive()) {
      wallpaper = null;
      style = "";
      return;
    }
    let disposed = false;
    let unlistenMove: (() => void) | null = null;
    let unlistenResize: (() => void) | null = null;
    let unlistenScale: (() => void) | null = null;
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    let refreshInFlight = false;

    const refreshWallpaper = async () => {
      if (disposed || refreshInFlight) return;
      refreshInFlight = true;
      try {
        const snapshot = await invoke<WallpaperSnapshot | null>("get_desktop_wallpaper");
        if (disposed) return;
        wallpaper = snapshot;
        style = "";
        if (snapshot) await realign();
      } catch {
        if (!disposed) {
          wallpaper = null;
          style = "";
        }
      } finally {
        refreshInFlight = false;
      }
    };
    // A resize or a DPI change moves the window *and* can land it on another
    // monitor, so re-align now and re-snapshot once the gesture settles.
    const scheduleRefresh = () => {
      void realign();
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => void refreshWallpaper(), RESIZE_SETTLE_MS);
    };

    void refreshWallpaper();
    const appWindow = getCurrentWindow();
    // Each listener resolves asynchronously, so one that lands after teardown
    // has to unsubscribe itself.
    void appWindow
      .onMoved(() => void realign())
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenMove = unlisten;
      });
    void appWindow.onResized(scheduleRefresh).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenResize = unlisten;
    });
    void appWindow.onScaleChanged(scheduleRefresh).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenScale = unlisten;
    });
    const refreshInterval = setInterval(() => void refreshWallpaper(), REFRESH_MS);

    return () => {
      disposed = true;
      if (resizeTimer) clearTimeout(resizeTimer);
      clearInterval(refreshInterval);
      unlistenMove?.();
      unlistenResize?.();
      unlistenScale?.();
    };
  });

  return {
    /** Null until the first snapshot lands, and whenever one fails. */
    get wallpaper() {
      return wallpaper;
    },
    /** Inline `background-size` / `background-position` for the backdrop layer. */
    get style() {
      return style;
    },
  };
}
