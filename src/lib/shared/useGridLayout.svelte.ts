export const CARD_WIDTH = 100;
export const GAP = 10;

export function createGridLayout() {
  let wrapperRef = $state<HTMLDivElement | null>(null);
  let paddingLeft = $state(0);
  let isResizing = $state(false);
  let resizeTimeout: number;
  let frameId: number | null = null;

  function calculatePadding() {
    if (!wrapperRef) return;
    const availableWidth = wrapperRef.clientWidth;
    const cardsPerRow = Math.floor((availableWidth + GAP) / (CARD_WIDTH + GAP));
    if (cardsPerRow < 1) return;
    const totalCardsWidth = cardsPerRow * CARD_WIDTH + (cardsPerRow - 1) * GAP;
    paddingLeft = Math.floor((availableWidth - totalCardsWidth) / 2);
  }

  function queueCalculatePadding() {
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
