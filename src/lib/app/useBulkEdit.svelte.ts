type BulkEditBarType = (typeof import("$lib/platforms/steam/BulkEditBar.svelte"))["default"];

type BulkEditDeps = {
  getCurrentAccountId: () => string | null;
  getVisibleAccountIds: () => string[];
};

export function createBulkEditController({
  getCurrentAccountId,
  getVisibleAccountIds,
}: BulkEditDeps) {
  let bulkEditMode = $state(false);
  let bulkEditSelectedIds = $state<Set<string>>(new Set());
  let BulkEditBar = $state<BulkEditBarType | null>(null);
  let bulkEditBarLoadPromise: Promise<void> | null = null;

  // Paint selection: press on a card, drag over others to select/deselect them.
  // The direction is locked for the whole gesture by the start card's state
  // (start on unselected -> select; start on selected -> deselect). We only
  // ever apply that one action until the button is released.
  const PAINT_THRESHOLD = 5;
  let painting = false;
  let paintDesired = false;
  let paintMoved = false;
  let paintStartX = 0;
  let paintStartY = 0;
  let paintStartId: string | null = null;
  let eatNextClick = false;

  function accountIdFromPoint(x: number, y: number): string | null {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const card = el?.closest("[data-account-id]") as HTMLElement | null;
    return card?.dataset.accountId ?? null;
  }

  function setBulkEditAccountSelected(accountId: string, selected: boolean) {
    if (selected === bulkEditSelectedIds.has(accountId)) return;
    const next = new Set(bulkEditSelectedIds);
    if (selected) next.add(accountId);
    else next.delete(accountId);
    bulkEditSelectedIds = next;
  }

  let bulkEditActiveAccountSelected = $derived(
    bulkEditSelectedIds.size > 0 &&
      !!getCurrentAccountId() &&
      bulkEditSelectedIds.has(getCurrentAccountId()!),
  );

  function toggleBulkEdit() {
    if (bulkEditMode) {
      bulkEditMode = false;
      bulkEditSelectedIds = new Set();
      return;
    }
    if (BulkEditBar) {
      bulkEditMode = true;
      return;
    }
    if (!bulkEditBarLoadPromise) {
      bulkEditBarLoadPromise = import("$lib/platforms/steam/BulkEditBar.svelte")
        .then((mod) => {
          BulkEditBar = mod.default;
          bulkEditMode = true;
        })
        .catch(() => {
          bulkEditBarLoadPromise = null;
        });
    }
  }

  function toggleBulkEditAccount(accountId: string) {
    const next = new Set(bulkEditSelectedIds);
    if (next.has(accountId)) next.delete(accountId);
    else next.add(accountId);
    bulkEditSelectedIds = next;
  }

  function bulkEditSelectAll() {
    bulkEditSelectedIds = new Set(getVisibleAccountIds());
  }

  function bulkEditDeselectAll() {
    bulkEditSelectedIds = new Set();
  }

  function closeBulkEdit() {
    bulkEditMode = false;
    bulkEditSelectedIds = new Set();
  }

  // Arm the paint gesture. Returns true if a card was pressed (so the caller
  // knows to skip the reorder drag manager and keep the cards locked in place).
  function handlePaintMouseDown(e: MouseEvent): boolean {
    if (!bulkEditMode || e.button !== 0) return false;
    const card = (e.target as HTMLElement).closest("[data-account-id]") as HTMLElement | null;
    const accountId = card?.dataset.accountId;
    if (!accountId) return false;
    paintStartId = accountId;
    paintDesired = !bulkEditSelectedIds.has(accountId);
    paintStartX = e.clientX;
    paintStartY = e.clientY;
    paintMoved = false;
    painting = true;
    return true;
  }

  function handlePaintMouseMove(e: MouseEvent) {
    if (!painting) return;
    if (!paintMoved) {
      if (Math.abs(e.clientX - paintStartX) + Math.abs(e.clientY - paintStartY) < PAINT_THRESHOLD) {
        return;
      }
      paintMoved = true;
      if (paintStartId) setBulkEditAccountSelected(paintStartId, paintDesired);
    }
    const id = accountIdFromPoint(e.clientX, e.clientY);
    if (id) setBulkEditAccountSelected(id, paintDesired);
  }

  function handlePaintMouseUp() {
    if (!painting) return;
    const moved = paintMoved;
    painting = false;
    paintMoved = false;
    paintStartId = null;
    // A real drag already applied the selection; swallow the trailing click so
    // it does not toggle the card back. A pure click (no move) falls through to
    // the normal single-click toggle.
    if (moved) {
      eatNextClick = true;
      setTimeout(() => {
        eatNextClick = false;
      }, 0);
    }
  }

  function handlePaintCaptureClick(e: MouseEvent) {
    if (eatNextClick) {
      e.stopPropagation();
      e.preventDefault();
      eatNextClick = false;
    }
  }

  return {
    get bulkEditMode() {
      return bulkEditMode;
    },
    get bulkEditSelectedIds() {
      return bulkEditSelectedIds;
    },
    get BulkEditBar() {
      return BulkEditBar;
    },
    get bulkEditActiveAccountSelected() {
      return bulkEditActiveAccountSelected;
    },
    toggleBulkEdit,
    toggleBulkEditAccount,
    bulkEditSelectAll,
    bulkEditDeselectAll,
    closeBulkEdit,
    handlePaintMouseDown,
    handlePaintMouseMove,
    handlePaintMouseUp,
    handlePaintCaptureClick,
  };
}
