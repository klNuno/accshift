export const CARD_WIDTH = 100;
export const GAP = 10;

/**
 * Card width in px for a density scale. Pure, so the grid math stays
 * unit-testable. Matches `--grid-card-width: calc(100px * var(--density-scale))`
 * in app.css (compact 0.88, cozy 1, comfortable 1.12).
 */
export function cardWidthForDensity(densityScale: number): number {
  const scale = Number.isFinite(densityScale) && densityScale > 0 ? densityScale : 1;
  return CARD_WIDTH * scale;
}

/** Live card width: reads the --density-scale the theme actually applied. */
function liveCardWidth(): number {
  if (typeof document === "undefined") return CARD_WIDTH;
  const raw = getComputedStyle(document.documentElement).getPropertyValue("--density-scale");
  return cardWidthForDensity(Number.parseFloat(raw));
}

/**
 * Flips at the webview's first frame. Until then an animation frame is far
 * away: the compositor waits for the GPU process to bring up its display,
 * well over 100 ms with some drivers, and the page has nothing to do meanwhile.
 */
let firstFrameDone = typeof requestAnimationFrame !== "function";
if (!firstFrameDone) {
  requestAnimationFrame(() => {
    firstFrameDone = true;
  });
}

export function createGridLayout() {
  let wrapperRef = $state<HTMLDivElement | null>(null);
  let paddingLeft = $state(0);
  let isResizing = $state(false);
  let resizeTimeout: number;
  let frameId: number | null = null;
  let taskId: number | null = null;

  function calculatePadding() {
    if (!wrapperRef) return;
    const cardWidth = liveCardWidth();
    const availableWidth = wrapperRef.clientWidth;
    const cardsPerRow = Math.floor((availableWidth + GAP) / (cardWidth + GAP));
    if (cardsPerRow < 1) return;
    const totalCardsWidth = cardsPerRow * cardWidth + (cardsPerRow - 1) * GAP;
    paddingLeft = Math.floor((availableWidth - totalCardsWidth) / 2);
  }

  function queueCalculatePadding() {
    if (!firstFrameDone) {
      // Reading the wrapper width lays out every card, and the first layout
      // is the costly one: it opens the fonts the cards use. Waiting for a
      // frame put that layout inside the first frame; a task runs it during
      // the GPU wait above, so the first frame only paints. The task still
      // comes after Svelte has flushed the new cards.
      if (taskId !== null) clearTimeout(taskId);
      taskId = window.setTimeout(() => {
        taskId = null;
        calculatePadding();
      }, 0);
      return;
    }
    if (frameId !== null) cancelAnimationFrame(frameId);
    frameId = requestAnimationFrame(() => {
      frameId = null;
      calculatePadding();
    });
  }

  function handleResize() {
    if (!isResizing) {
      isResizing = true;
      queueCalculatePadding();
    }
    clearTimeout(resizeTimeout);
    resizeTimeout = setTimeout(() => {
      isResizing = false;
      queueCalculatePadding();
    }, 120);
  }

  function destroy() {
    clearTimeout(resizeTimeout);
    if (taskId !== null) {
      clearTimeout(taskId);
      taskId = null;
    }
    if (frameId !== null) {
      cancelAnimationFrame(frameId);
      frameId = null;
    }
  }

  return {
    get wrapperRef() {
      return wrapperRef;
    },
    set wrapperRef(v: HTMLDivElement | null) {
      wrapperRef = v;
    },
    get paddingLeft() {
      return paddingLeft;
    },
    get isResizing() {
      return isResizing;
    },
    calculatePadding,
    queueCalculatePadding,
    handleResize,
    destroy,
  };
}
