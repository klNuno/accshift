import type { ItemRef } from "../features/folders/types";
import { moveItem, reorderItems, getFolder } from "../features/folders/store";

const DRAG_THRESHOLD = 5;

export interface DragState {
  dragItem: ItemRef | null;
  dragOverFolderId: string | null;
  dragOverBack: boolean;
  isDragging: boolean;
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
  let pendingDrag = $state<{ item: ItemRef; startX: number; startY: number } | null>(null);
  let eatNextClick = false;

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

    pendingDrag = { item, startX: e.clientX, startY: e.clientY };
  }

  function handleDocMouseMove(e: MouseEvent) {
    if (!pendingDrag) return;

    const dx = e.clientX - pendingDrag.startX;
    const dy = e.clientY - pendingDrag.startY;

    if (!isDragging) {
      if (Math.abs(dx) + Math.abs(dy) < DRAG_THRESHOLD) return;
      isDragging = true;
      dragItem = pendingDrag.item;
    }

    const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
    if (!el) {
      dragOverFolderId = null;
      dragOverBack = false;
      return;
    }

    const hover = el.closest("[data-folder-id], [data-back-card], [data-account-id]") as HTMLElement | null;

    if (hover?.dataset.backCard) {
      dragOverBack = true;
      dragOverFolderId = null;
    } else if (hover?.dataset.folderId && !(dragItem?.type === "folder" && dragItem?.id === hover.dataset.folderId)) {
      dragOverFolderId = hover.dataset.folderId!;
      dragOverBack = false;
    } else {
      dragOverFolderId = null;
      dragOverBack = false;
    }
  }

  function handleDocMouseUp(e: MouseEvent) {
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
      } else {
        performReorder(e.clientX, e.clientY);
      }
      options.onRefresh();
    }

    eatNextClick = true;
    dragItem = null;
    dragOverFolderId = null;
    dragOverBack = false;
    isDragging = false;
    pendingDrag = null;
  }

  function performReorder(x: number, y: number) {
    if (!dragItem) return;
    const wrapperRef = options.getWrapperRef();
    const gridEl = wrapperRef?.querySelector(".grid-container") as HTMLElement;
    if (!gridEl) return;

    const currentFolderId = options.getCurrentFolderId();
    const activeTab = options.getActiveTab();
    const folderItems = options.getFolderItems();
    const accountItems = options.getAccountItems();

    if (dragItem.type === "folder") {
      const folderCards = Array.from(gridEl.querySelectorAll(":scope > [data-folder-id]")) as HTMLElement[];
      const dropIdx = findDropIndex(x, y, folderCards);
      const newFolders = [...folderItems];
      const oldIdx = newFolders.findIndex(i => i.id === dragItem!.id);
      if (oldIdx === -1) return;
      newFolders.splice(oldIdx, 1);
      let insertAt = dropIdx > oldIdx ? dropIdx - 1 : dropIdx;
      insertAt = Math.max(0, Math.min(insertAt, newFolders.length));
      newFolders.splice(insertAt, 0, dragItem);
      reorderItems(currentFolderId, activeTab, [...newFolders, ...accountItems]);
    } else {
      const accountCards = Array.from(gridEl.querySelectorAll(":scope > [data-account-id]")) as HTMLElement[];
      const dropIdx = findDropIndex(x, y, accountCards);
      const newAccounts = [...accountItems];
      const oldIdx = newAccounts.findIndex(i => i.id === dragItem!.id);
      if (oldIdx === -1) return;
      newAccounts.splice(oldIdx, 1);
      let insertAt = dropIdx > oldIdx ? dropIdx - 1 : dropIdx;
      insertAt = Math.max(0, Math.min(insertAt, newAccounts.length));
      newAccounts.splice(insertAt, 0, dragItem);
      reorderItems(currentFolderId, activeTab, [...folderItems, ...newAccounts]);
    }
  }

  function findDropIndex(x: number, y: number, cards: HTMLElement[]): number {
    if (cards.length === 0) return 0;
    let bestIdx = 0;
    let bestDist = Infinity;
    for (let i = 0; i < cards.length; i++) {
      const r = cards[i].getBoundingClientRect();
      const dist = (x - (r.left + r.width / 2)) ** 2 + (y - (r.top + r.height / 2)) ** 2;
      if (dist < bestDist) { bestDist = dist; bestIdx = i; }
    }
    const r = cards[bestIdx].getBoundingClientRect();
    return x > r.left + r.width / 2 ? bestIdx + 1 : bestIdx;
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
    handleGridMouseDown,
    handleDocMouseMove,
    handleDocMouseUp,
    handleCaptureClick,
  };
}
