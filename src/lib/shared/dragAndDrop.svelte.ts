import type { ItemRef } from "../features/folders/types";
import { moveItem, reorderItems, getFolder } from "../features/folders/store";

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
  let pendingDrag = $state<{ item: ItemRef; startX: number; startY: number; sourceEl: HTMLElement } | null>(null);
  // Swallow the click generated right after a drag so we do not trigger card actions.
  let eatNextClick = false;
  let ghostEl: HTMLElement | null = null;
  let ghostOffsetX = 0;
  let ghostOffsetY = 0;
  let lastClientX = 0;
  let lastClientY = 0;
  let hasPointer = false;

  // Snapshot card slots at drag start to keep preview calculations stable while DOM reorders.
  let slotRects: DOMRect[] = [];
  let dragOldIndex = -1;
  let dragIsListMode = false;

  function refreshSlotRects() {
    if (!dragItem) return;
    const wrapperRef = options.getWrapperRef();
    const gridEl = wrapperRef?.querySelector(".grid-container, .list-container") as HTMLElement | null;
    if (!gridEl) {
      slotRects = [];
      dragOldIndex = -1;
      return;
    }
    dragIsListMode = gridEl.classList.contains("list-container");
    const selector = dragItem.type === "folder" ? "[data-folder-id]" : "[data-account-id]";
    const cards = Array.from(gridEl.querySelectorAll(selector)) as HTMLElement[];
    slotRects = cards.map(el => el.getBoundingClientRect());
    const items = dragItem.type === "folder" ? options.getFolderItems() : options.getAccountItems();
    dragOldIndex = items.findIndex(i => i.id === dragItem!.id);
  }

  function updateDragAt(clientX: number, clientY: number) {
    if (!isDragging) return;

    if (ghostEl) {
      ghostEl.style.left = `${clientX - ghostOffsetX}px`;
      ghostEl.style.top = `${clientY - ghostOffsetY}px`;
    }

    const el = document.elementFromPoint(clientX, clientY) as HTMLElement | null;
    if (!el) {
      dragOverFolderId = null;
      dragOverBack = false;
      previewIndex = null;
      return;
    }

    const hover = el.closest("[data-folder-id], [data-back-card], [data-account-id]") as HTMLElement | null;

    if (hover?.dataset.backCard) {
      dragOverBack = true;
      dragOverFolderId = null;
      previewIndex = null;
    } else if (hover?.dataset.folderId && !(dragItem?.type === "folder" && dragItem?.id === hover.dataset.folderId)) {
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
          const dist = (clientX - (r.left + r.width / 2)) ** 2 + (clientY - (r.top + r.height / 2)) ** 2;
          if (dist < bestDist) { bestDist = dist; bestIdx = i; }
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

    lastClientX = e.clientX;
    lastClientY = e.clientY;
    hasPointer = true;
    pendingDrag = { item, startX: e.clientX, startY: e.clientY, sourceEl: card };
  }

  function handleDocMouseMove(e: MouseEvent) {
    if (!pendingDrag) return;
    lastClientX = e.clientX;
    lastClientY = e.clientY;
    hasPointer = true;

    const dx = e.clientX - pendingDrag.startX;
    const dy = e.clientY - pendingDrag.startY;

    if (!isDragging) {
      if (Math.abs(dx) + Math.abs(dy) < DRAG_THRESHOLD) return;
      isDragging = true;
      dragItem = pendingDrag.item;

      // Capture slot positions once, before preview reorders mutate the DOM layout.
      refreshSlotRects();

      // Render a floating clone so the dragged card stays visible under the cursor.
      const sourceRect = pendingDrag.sourceEl.getBoundingClientRect();
      ghostOffsetX = e.clientX - sourceRect.left;
      ghostOffsetY = e.clientY - sourceRect.top;
      ghostEl = pendingDrag.sourceEl.cloneNode(true) as HTMLElement;
      ghostEl.style.position = "fixed";
      ghostEl.style.left = `${sourceRect.left}px`;
      ghostEl.style.top = `${sourceRect.top}px`;
      ghostEl.style.width = `${sourceRect.width}px`;
      ghostEl.style.pointerEvents = "none";
      ghostEl.style.zIndex = "9999";
      ghostEl.style.opacity = "0.85";
      ghostEl.style.transform = "scale(1.05)";
      ghostEl.style.boxShadow = "0 8px 24px rgba(0,0,0,0.5)";
      ghostEl.style.transition = "none";
      ghostEl.style.margin = "0";
      document.body.appendChild(ghostEl);
    }

    updateDragAt(e.clientX, e.clientY);
  }

  function handleDocScroll() {
    if (!isDragging || !hasPointer) return;
    refreshSlotRects();
    updateDragAt(lastClientX, lastClientY);
  }

  function handleDocMouseUp() {
    if (!pendingDrag) return;

    if (!isDragging) {
      pendingDrag = null;
      return;
    }

    const currentFolderId = options.getCurrentFolderId();
    const activeTab = options.getActiveTab();

    if (dragItem) {
      if (dragOverBack && currentFolderId) {
        const current = getFolder(currentFolderId);
        moveItem(dragItem, currentFolderId, current?.parentId ?? null, activeTab);
      } else if (dragOverFolderId) {
        if (!(dragItem.type === "folder" && dragItem.id === dragOverFolderId)) {
          moveItem(dragItem, currentFolderId, dragOverFolderId, activeTab);
        }
      } else if (previewIndex !== null) {
        // Commit the same order shown by the preview.
        const folderItems = options.getFolderItems();
        const accountItems = options.getAccountItems();
        if (dragItem.type === "folder") {
          const newFolders = folderItems.filter(i => i.id !== dragItem!.id);
          newFolders.splice(Math.min(previewIndex, newFolders.length), 0, dragItem);
          reorderItems(currentFolderId, activeTab, [...newFolders, ...accountItems]);
        } else {
          const newAccounts = accountItems.filter(i => i.id !== dragItem!.id);
          newAccounts.splice(Math.min(previewIndex, newAccounts.length), 0, dragItem);
          reorderItems(currentFolderId, activeTab, [...folderItems, ...newAccounts]);
        }
      }
      options.onRefresh();
    }

    // Always cleanup drag ghost when the interaction ends.
    if (ghostEl) {
      ghostEl.remove();
      ghostEl = null;
    }

    eatNextClick = true;
    // If no click event is emitted, clear the guard on the next tick.
    setTimeout(() => { eatNextClick = false; }, 0);
    dragItem = null;
    dragOverFolderId = null;
    dragOverBack = false;
    isDragging = false;
    pendingDrag = null;
    previewIndex = null;
    slotRects = [];
    dragOldIndex = -1;
    hasPointer = false;
  }

  function handleCaptureClick(e: MouseEvent) {
    if (eatNextClick) {
      e.stopPropagation();
      e.preventDefault();
      eatNextClick = false;
    }
  }

  return {
    get dragItem() { return dragItem; },
    get dragOverFolderId() { return dragOverFolderId; },
    get dragOverBack() { return dragOverBack; },
    get isDragging() { return isDragging; },
    get previewIndex() { return previewIndex; },
    handleGridMouseDown,
    handleDocMouseMove,
    handleDocScroll,
    handleDocMouseUp,
    handleCaptureClick,
  };
}
