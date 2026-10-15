import type { ItemRef } from "$lib/features/folders/types";
import { cardsPerRow, liveCardWidth } from "$lib/shared/useGridLayout.svelte";

type CardFocusDeps = {
  /** Cards in visual order (folders then accounts, sections flattened). */
  getItems: () => ItemRef[];
  getWrapperRef: () => HTMLElement | null;
  getViewMode: () => string;
};

function cardSelector(item: ItemRef): string {
  return item.type === "folder"
    ? `[data-folder-id="${CSS.escape(item.id)}"]`
    : `[data-account-id="${CSS.escape(item.id)}"]`;
}

const CARD_SELECTOR = "[data-account-id], [data-folder-id]";
const INTERACTIVE_SELECTOR = [
  "button",
  "a[href]",
  "input",
  "select",
  "textarea",
  "summary",
  '[contenteditable]:not([contenteditable="false"])',
  '[role="button"]',
  '[role="link"]',
  '[role="menuitem"]',
  '[role="checkbox"]',
  '[role="switch"]',
  '[role="tab"]',
  '[role="option"]',
].join(", ");

function isPageRoot(target: Element): boolean {
  const doc = target.ownerDocument;
  return target === doc?.body || target === doc?.documentElement;
}

/** Whether Enter or Space aimed at `target` belongs to the grid's virtual
 *  focus. Real focus on a control (titlebar, bulk edit bar, search, a button
 *  inside a card) keeps its native activation; so does anything outside the
 *  grid. A card that holds real focus defers to the virtual one, which is
 *  what the arrow keys moved last. */
export function isCardKeyTarget(target: EventTarget | null, wrapper: Element | null): boolean {
  if (!(target instanceof Element)) return true;
  if (isPageRoot(target)) return true;
  if (!wrapper || !wrapper.contains(target)) return false;
  if (target.matches(CARD_SELECTOR)) return true;
  return !target.matches(INTERACTIVE_SELECTOR);
}

/** Virtual roving focus for the card grid. The cards are managed by the drag
 *  manager and stay plain divs, so instead of DOM focus we track a focused
 *  item id and paint it via a data attribute (styled globally in app.css).
 *  Arrow keys move it, Enter/menu keys act on it from App-level bindings. */
export function createCardFocus({ getItems, getWrapperRef, getViewMode }: CardFocusDeps) {
  let focusedId = $state<string | null>(null);
  /** Whether a card currently carries the data-kb-focus mark. Lets syncDom
   *  bail before querying the whole grid when there is nothing to paint and
   *  nothing to clear, which is every list change for mouse-only users. */
  let hasMark = false;

  function findElement(item: ItemRef): HTMLElement | null {
    const wrapper = getWrapperRef();
    return wrapper?.querySelector<HTMLElement>(cardSelector(item)) ?? null;
  }

  function focusedItem(): ItemRef | null {
    if (!focusedId) return null;
    return getItems().find((item) => item.id === focusedId) ?? null;
  }

  function syncDom() {
    if (focusedId === null && !hasMark) return;
    const wrapper = getWrapperRef();
    if (!wrapper) return;
    for (const el of wrapper.querySelectorAll<HTMLElement>("[data-kb-focus]")) {
      delete el.dataset.kbFocus;
    }
    hasMark = false;
    const item = focusedItem();
    if (!item) return;
    const el = findElement(item);
    if (!el) return;
    el.dataset.kbFocus = "true";
    hasMark = true;
    el.scrollIntoView({ block: "nearest" });
  }

  /** Cards per row of the grid holding `item`, from its live width so it
   *  follows zoom, density and resize. Each section of the sections layout
   *  has its own grid, and one with a single card still wraps at the full
   *  width. List view is always a single column. */
  function measureColumns(item: ItemRef): number {
    if (getViewMode() === "list") return 1;
    const grid = findElement(item)?.closest<HTMLElement>(".grid-container");
    if (!grid) return 1;
    const style = getComputedStyle(grid);
    const width =
      grid.clientWidth -
      Number.parseFloat(style.paddingLeft) -
      Number.parseFloat(style.paddingRight);
    return Math.max(1, cardsPerRow(width, liveCardWidth()));
  }

  function setFocused(id: string | null) {
    focusedId = id;
    syncDom();
  }

  /** Moves the virtual focus. Returns false when there is nothing to focus,
   *  so the caller can let the key fall through. */
  function move(direction: "left" | "right" | "up" | "down"): boolean {
    const items = getItems();
    if (items.length === 0) return false;
    const currentIndex = focusedId ? items.findIndex((item) => item.id === focusedId) : -1;
    if (currentIndex === -1) {
      setFocused(items[0].id);
      return true;
    }
    const columns = measureColumns(items[currentIndex]);
    const delta =
      direction === "left"
        ? -1
        : direction === "right"
          ? 1
          : direction === "up"
            ? -columns
            : columns;
    const next = currentIndex + delta;
    if (next < 0 || next >= items.length) return true;
    setFocused(items[next].id);
    return true;
  }

  function clear() {
    if (focusedId !== null) setFocused(null);
  }

  /** Drops the virtual focus when real focus lands outside the grid, so a
   *  highlighted card does not linger behind the control now in use. */
  function releaseIfOutside(target: EventTarget | null) {
    if (focusedId === null || !(target instanceof Element) || isPageRoot(target)) return;
    const wrapper = getWrapperRef();
    if (wrapper?.contains(target)) return;
    clear();
  }

  return {
    get focusedId() {
      return focusedId;
    },
    get focusedItem() {
      return focusedItem();
    },
    findElement,
    setFocused,
    move,
    clear,
    releaseIfOutside,
    ownsActivationKey: (target: EventTarget | null) => isCardKeyTarget(target, getWrapperRef()),
    syncDom,
  };
}
