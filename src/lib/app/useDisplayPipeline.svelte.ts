import type { PlatformAccount } from "$lib/shared/platform";
import type { ItemRef } from "$lib/features/folders/types";

type DisplayPipelineDeps = {
  navigation: {
    readonly isSearching: boolean;
    readonly searchQuery: string;
    readonly folderItems: ItemRef[];
    readonly accountItems: ItemRef[];
  };
  drag: {
    readonly isDragging: boolean;
    readonly dragItem: ItemRef | null;
    readonly previewIndex: number | null;
  };
  loader: {
    readonly accounts: PlatformAccount[];
    readonly accountMap: Record<string, PlatformAccount>;
  };
  addFlow: {
    readonly pendingSetupAccount: PlatformAccount | null;
  };
};

export function matchesSearch(account: PlatformAccount, query: string): boolean {
  return (
    account.id.toLowerCase().includes(query) ||
    account.username.toLowerCase().includes(query) ||
    (account.displayName || "").toLowerCase().includes(query)
  );
}

export function createDisplayPipeline(deps: DisplayPipelineDeps) {
  const { navigation, drag, loader, addFlow } = deps;

  let displayFolderItems = $derived.by(() => {
    if (navigation.isSearching) return [] as ItemRef[];
    if (
      !drag.isDragging ||
      !drag.dragItem ||
      drag.dragItem.type !== "folder" ||
      drag.previewIndex === null
    ) {
      return navigation.folderItems;
    }
    const arr = navigation.folderItems.filter((i) => i.id !== drag.dragItem!.id);
    arr.splice(Math.min(drag.previewIndex, arr.length), 0, drag.dragItem);
    return arr;
  });

  let filteredAccountItems = $derived.by(() => {
    const q = navigation.searchQuery.trim().toLowerCase();
    if (!q) return navigation.accountItems;
    return loader.accounts
      .filter((a) => matchesSearch(a, q))
      .map((a) => ({ type: "account" as const, id: a.id }));
  });

  let displayAccountItems = $derived.by(() => {
    if (navigation.isSearching) return filteredAccountItems;
    if (
      !drag.isDragging ||
      !drag.dragItem ||
      drag.dragItem.type !== "account" ||
      drag.previewIndex === null
    ) {
      return filteredAccountItems;
    }
    const arr = filteredAccountItems.filter((i) => i.id !== drag.dragItem!.id);
    arr.splice(Math.min(drag.previewIndex, arr.length), 0, drag.dragItem);
    return arr;
  });

  let displayAccountItemsWithPending = $derived.by(() => {
    const pending = addFlow.pendingSetupAccount;
    if (!pending) return displayAccountItems;
    if (displayAccountItems.some((item) => item.type === "account" && item.id === pending.id)) {
      return displayAccountItems;
    }
    return [...displayAccountItems, { type: "account" as const, id: pending.id }];
  });

  let visibleRenderedAccountIds = $derived.by(() => {
    const ids: string[] = [];
    const seen = new Set<string>();
    for (const item of displayAccountItemsWithPending) {
      if (item.type !== "account" || seen.has(item.id)) continue;
      seen.add(item.id);
      ids.push(item.id);
    }
    return ids;
  });

  let renderedAccountMap = $derived.by(() => {
    const pending = addFlow.pendingSetupAccount;
    if (!pending) return loader.accountMap;
    return {
      ...loader.accountMap,
      [pending.id]: pending,
    };
  });

  let renderedAccountCount = $derived.by(() => {
    const pending = addFlow.pendingSetupAccount;
    return loader.accounts.length + (pending && !loader.accountMap[pending.id] ? 1 : 0);
  });

  return {
    get displayFolderItems() {
      return displayFolderItems;
    },
    get filteredAccountItems() {
      return filteredAccountItems;
    },
    get displayAccountItems() {
      return displayAccountItems;
    },
    get displayAccountItemsWithPending() {
      return displayAccountItemsWithPending;
    },
    get visibleRenderedAccountIds() {
      return visibleRenderedAccountIds;
    },
    get renderedAccountMap() {
      return renderedAccountMap;
    },
    get renderedAccountCount() {
      return renderedAccountCount;
    },
  };
}
