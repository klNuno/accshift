import type { ItemRef } from "../features/folders/types";
import { moveItem, reorderItems, getFolder, isFolderDescendant } from "../features/folders/store";

const DRAG_THRESHOLD = 5;

export interface DragState {
  dragItem: ItemRef | null;
  dragOverFolderId: string | null;
  dragOverBack: boolean;
  isDragging: boolean;
  previewIndex: number | null;
}

export interface DragManagerOptions {
  getCurrentFolderId: () => string | null;
  getActiveTab: () => string;
  getFolderItems: () => ItemRef[];
  getAccountItems: () => ItemRef[];
  getWrapperRef: () => HTMLDivElement | null;
  onRefresh: () => void;
}

export function createDragManager(options: DragManagerOptions) {
  let dragItem = $state<ItemRef | null>(null);
  let dragOverFolderId = $state<string | null>(null);
  let dragOverBack = $state(false);
  let isDragging = $state(false);
  let previewIndex = $state<number | null>(null);
  let pendingDrag = $state<{
    item: ItemRef;
    startX: number;
    startY: number;
    sourceEl: HTMLElement;
  } | null>(null);
  // Swallow the click generated right after a drag so we do not trigger card actions.
  let eatNextClick = false;
  let ghostEl: HTMLElement | null = null;
  let ghostOffsetX = 0;
  let ghostOffsetY = 0;
  let lastClientX = 0;
  let lastClientY = 0;
  let hasPointer = false;
  let dragRafId: number | null = null;
  // Set when Escape cancels an active drag, so the release mouseup and its click are swallowed.
  let dragCancelled = false;
  let dragInSectionsMode = false;
  let pendingRectRefresh = false;

  // Snapshot card slots at drag start to keep preview calculations stable while DOM reorders.
  let slotRects: DOMRect[] = [];
  let dragOldIndex = -1;
  let dragIsListMode = false;

  function refreshSlotRects() {
    if (!dragItem) return;
    const wrapperRef = options.getWrapperRef();
    const sectionsEl = wrapperRef?.querySelector("[data-sections-mode]") as HTMLElement | null;
    dragInSectionsMode = !!sectionsEl;

    if (sectionsEl && dragItem.type === "folder") {
      const cards = Array.from(
        sectionsEl.querySelectorAll(':scope > .section[data-section-card="true"]'),
      ) as HTMLElement[];
      slotRects = cards.map((el) => el.getBoundingClientRect());
      dragIsListMode = true;
      const items = options.getFolderItems();
      dragOldIndex = items.findIndex((i) => i.id === dragItem!.id);
      return;
    }

    if (sectionsEl && dragItem.type === "account") {
      // Account reorder is not supported in sections mode; only move-into-folder via dragOverFolderId.
      slotRects = [];
      dragOldIndex = -1;
      return;
    }

    const gridEl = wrapperRef?.querySelector(
      ".grid-container, .list-container",
    ) as HTMLElement | null;
    if (!gridEl) {
      slotRects = [];
      dragOldIndex = -1;
      return;
    }
    dragIsListMode = gridEl.classList.contains("list-container");
    const selector = dragItem.type === "folder" ? "[data-folder-id]" : "[data-account-id]";
    const cards = Array.from(gridEl.querySelectorAll(selector)) as HTMLElement[];
    slotRects = cards.map((el) => el.getBoundingClientRect());
    const items = dragItem.type === "folder" ? options.getFolderItems() : options.getAccountItems();
    dragOldIndex = items.findIndex((i) => i.id === dragItem!.id);
  }

  function updateDragAt(clientX: number, clientY: number) {
    if (!isDragging) return;

    // Hit-test before touching the ghost style so the layout read does not
    // force a reflow right after a write in the same frame.
    const el = document.elementFromPoint(clientX, clientY) as HTMLElement | null;

    if (ghostEl) {
      ghostEl.style.left = `${clientX - ghostOffsetX}px`;
      ghostEl.style.top = `${clientY - ghostOffsetY}px`;
    }

    if (!el) {
      dragOverFolderId = null;
      dragOverBack = false;
      previewIndex = null;
      return;
    }

    const hover = el.closest(
      "[data-folder-id], [data-back-card], [data-account-id]",
    ) as HTMLElement | null;
    const isSectionHover = hover?.dataset.sectionCard === "true";

    if (hover?.dataset.backCard) {
      dragOverBack = true;
      dragOverFolderId = null;
      previewIndex = null;
    } else if (
      hover?.dataset.folderId &&
      !(dragItem?.type === "folder" && isFolderDescendant(hover.dataset.folderId, dragItem.id)) &&
      !(dragInSectionsMode && dragItem?.type === "folder" && isSectionHover)
    ) {
      dragOverFolderId = hover.dataset.folderId!;
      dragOverBack = false;
      previewIndex = null;
    } else {
      dragOverFolderId = null;
      dragOverBack = false;
      if (slotRects.length > 0 && dragOldIndex >= 0) {
        let bestIdx = 0;
        let bestDist = Infinity;
        for (let i = 0; i < slotRects.length; i++) {
          const r = slotRects[i];
          const dist =
            (clientX - (r.left + r.width / 2)) ** 2 + (clientY - (r.top + r.height / 2)) ** 2;
          if (dist < bestDist) {
            bestDist = dist;
            bestIdx = i;
          }
        }
        const r = slotRects[bestIdx];
        let dropIdx: number;
        if (dragIsListMode) {
          dropIdx = clientY > r.top + r.height / 2 ? bestIdx + 1 : bestIdx;
        } else {
          dropIdx = clientX > r.left + r.width / 2 ? bestIdx + 1 : bestIdx;
        }
        const insertAt = dropIdx > dragOldIndex ? dropIdx - 1 : dropIdx;
        previewIndex = Math.max(0, Math.min(insertAt, slotRects.length - 1));
      }
    }
  }

  function handleGridMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    const card = target.closest("[data-account-id], [data-folder-id]") as HTMLElement | null;
    if (!card) return;

    let item: ItemRef | null = null;
    if (card.dataset.accountId) {
      item = { type: "account", id: card.dataset.accountId };
    } else if (card.dataset.folderId) {
      item = { type: "folder", id: card.dataset.folderId };
    }
    if (!item) return;

    // In sections mode, only top-level section wrappers are draggable as folders.
    // Subfolder cards inside sections stay click-to-navigate only.
    const wrapperRef = options.getWrapperRef();
    const inSectionsMode = !!wrapperRef?.querySelector("[data-sections-mode]");
    if (inSectionsMode && item.type === "folder" && card.dataset.sectionCard !== "true") {
      return;
    }

    lastClientX = e.clientX;
    lastClientY = e.clientY;
    hasPointer = true;
    dragCancelled = false;
    pendingDrag = { item, startX: e.clientX, startY: e.clientY, sourceEl: card };
    document.addEventListener("keydown", handleDragKeyDown, true);
  }

  function runDragFrame() {
    dragRafId = null;
    if (pendingRectRefresh) {
      pendingRectRefresh = false;
      refreshSlotRects();
    }
    updateDragAt(lastClientX, lastClientY);
  }

  function scheduleDragFrame() {
    if (dragRafId === null) {
      dragRafId = requestAnimationFrame(runDragFrame);
    }
  }

  function handleDocMouseMove(e: MouseEvent) {
    if (!pendingDrag) return;
    // The mouseup was missed (button released outside the window): abort without committing.
    if (e.buttons === 0) {
      cancelDrag();
      return;
    }
    lastClientX = e.clientX;
    lastClientY = e.clientY;
    hasPointer = true;

    const dx = e.clientX - pendingDrag.startX;
    const dy = e.clientY - pendingDrag.startY;

    if (!isDragging) {
      if (Math.abs(dx) + Math.abs(dy) < DRAG_THRESHOLD) return;
      isDragging = true;
      dragItem = pendingDrag.item;

      refreshSlotRects();

      const sourceRect = pendingDrag.sourceEl.getBoundingClientRect();
      ghostOffsetX = e.clientX - sourceRect.left;
      ghostOffsetY = e.clientY - sourceRect.top;
      ghostEl = pendingDrag.sourceEl.cloneNode(true) as HTMLElement;
      Object.assign(ghostEl.style, {
        position: "fixed",
        left: `${sourceRect.left}px`,
        top: `${sourceRect.top}px`,
        width: `${sourceRect.width}px`,
        pointerEvents: "none",
        zIndex: "9999",
        opacity: "0.85",
        transform: "scale(1.05)",
        boxShadow: "0 8px 24px rgba(0,0,0,0.5)",
        transition: "none",
        margin: "0",
      });
      // Inherit CSS custom properties from ancestors (e.g. --card-custom-color on .card-shell)
      const parentEl = pendingDrag.sourceEl.parentElement;
      if (parentEl) {
        const parentStyle = getComputedStyle(parentEl);
        const customColor = parentStyle.getPropertyValue("--card-custom-color").trim();
        if (customColor) {
          ghostEl.style.setProperty("--card-custom-color", customColor);
        }
        const folderColor = parentStyle.getPropertyValue("--folder-custom-color").trim();
        if (folderColor) {
          ghostEl.style.setProperty("--folder-custom-color", folderColor);
        }
      }
      document.body.appendChild(ghostEl);
    }

    scheduleDragFrame();
  }

  function handleDocScroll() {
    if (!isDragging || !hasPointer) return;
    // Slot rects are stale after a scroll; recompute at most once per frame.
    pendingRectRefresh = true;
    scheduleDragFrame();
  }

  function cancelDrag() {
    if (dragRafId !== null) {
      cancelAnimationFrame(dragRafId);
      dragRafId = null;
    }
    if (ghostEl) {
      ghostEl.remove();
      ghostEl = null;
    }
    dragItem = null;
    dragOverFolderId = null;
    dragOverBack = false;
    isDragging = false;
    pendingDrag = null;
    previewIndex = null;
    slotRects = [];
    dragOldIndex = -1;
    hasPointer = false;
    pendingRectRefresh = false;
    document.removeEventListener("keydown", handleDragKeyDown, true);
  }

  function handleDragKeyDown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (!pendingDrag && !isDragging) return;
    // Keep the Escape from also closing overlays while a drag is in flight.
    e.stopPropagation();
    if (isDragging) dragCancelled = true;
    cancelDrag();
  }

  function handleDocMouseUp() {
    if (dragCancelled) {
      // Release after an Escape cancel: swallow the mouseup and its click.
      dragCancelled = false;
      eatNextClick = true;
      setTimeout(() => {
        eatNextClick = false;
      }, 0);
      return;
    }

    if (!pendingDrag) return;

    if (!isDragging) {
      cancelDrag();
      return;
    }

    const currentFolderId = options.getCurrentFolderId();
    const activeTab = options.getActiveTab();
    // In sections mode, derive the actual source folder for account drags from the source section.
    let sourceFolderId = currentFolderId;
    if (dragInSectionsMode && dragItem?.type === "account" && pendingDrag) {
      const sectionEl = pendingDrag.sourceEl.closest(
        '[data-section-card="true"]',
      ) as HTMLElement | null;
      sourceFolderId = sectionEl?.dataset.folderId ?? null;
    }

    if (dragItem) {
      if (dragOverBack && currentFolderId) {
        const current = getFolder(currentFolderId);
        moveItem(dragItem, currentFolderId, current?.parentId ?? null, activeTab);
      } else if (dragOverFolderId) {
        if (!(dragItem.type === "folder" && dragItem.id === dragOverFolderId)) {
          if (sourceFolderId !== dragOverFolderId) {
            moveItem(dragItem, sourceFolderId, dragOverFolderId, activeTab);
          }
        }
      } else if (previewIndex !== null) {
        // Commit the same order shown by the preview.
        const folderItems = options.getFolderItems();
        const accountItems = options.getAccountItems();
        if (dragItem.type === "folder") {
          const newFolders = folderItems.filter((i) => i.id !== dragItem!.id);
          newFolders.splice(Math.min(previewIndex, newFolders.length), 0, dragItem);
          reorderItems(currentFolderId, activeTab, [...newFolders, ...accountItems]);
        } else {
          const newAccounts = accountItems.filter((i) => i.id !== dragItem!.id);
          newAccounts.splice(Math.min(previewIndex, newAccounts.length), 0, dragItem);
          reorderItems(currentFolderId, activeTab, [...folderItems, ...newAccounts]);
        }
      }
      options.onRefresh();
    }

    eatNextClick = true;
    // If no click event is emitted, clear the guard on the next tick.
    setTimeout(() => {
      eatNextClick = false;
    }, 0);
    cancelDrag();
  }

  function handleCaptureClick(e: MouseEvent) {
    if (eatNextClick) {
      e.stopPropagation();
      e.preventDefault();
      eatNextClick = false;
    }
  }

  return {
    get dragItem() {
      return dragItem;
    },
    get dragOverFolderId() {
      return dragOverFolderId;
    },
    get dragOverBack() {
      return dragOverBack;
    },
    get isDragging() {
      return isDragging;
    },
    get previewIndex() {
      return previewIndex;
    },
    handleGridMouseDown,
    handleDocMouseMove,
    handleDocScroll,
    handleDocMouseUp,
    handleCaptureClick,
  };
}
