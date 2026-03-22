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
  };
}
