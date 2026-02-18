export const CARD_WIDTH = 100;
export const GAP = 10;

export function createGridLayout() {
  let wrapperRef = $state<HTMLDivElement | null>(null);
  let paddingLeft = $state(0);
  let isResizing = $state(false);
  let resizeTimeout: number;

  function calculatePadding() {
    if (!wrapperRef) return;
    const availableWidth = wrapperRef.clientWidth;
    const cardsPerRow = Math.floor((availableWidth + GAP) / (CARD_WIDTH + GAP));
    if (cardsPerRow < 1) return;
    const totalCardsWidth = cardsPerRow * CARD_WIDTH + (cardsPerRow - 1) * GAP;
    paddingLeft = Math.floor((availableWidth - totalCardsWidth) / 2);
  }

  function handleResize() {
    isResizing = true;
    clearTimeout(resizeTimeout);
    resizeTimeout = setTimeout(() => { isResizing = false; calculatePadding(); }, 200);
  }

  function destroy() {
    clearTimeout(resizeTimeout);
  }

  return {
    get wrapperRef() { return wrapperRef; },
    set wrapperRef(v: HTMLDivElement | null) { wrapperRef = v; },
    get paddingLeft() { return paddingLeft; },
    get isResizing() { return isResizing; },
    calculatePadding,
    handleResize,
    destroy,
  };
}
