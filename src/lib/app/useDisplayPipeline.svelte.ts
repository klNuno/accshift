import type { PlatformAccount } from "$lib/shared/platform";
import type { ItemRef } from "$lib/features/folders/types";
import { getRootSections, type FolderSection } from "$lib/features/folders/store";

type DisplayPipelineDeps = {
  navigation: {
    readonly isSearching: boolean;
    readonly searchQuery: string;
    readonly folderItems: ItemRef[];
    readonly accountItems: ItemRef[];
    readonly currentFolderId: string | null;
    readonly currentItems: ItemRef[];
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
  getExpandedFolders: () => boolean;
  getActiveTab: () => string;
};

export type DisplaySection = {
  folder: FolderSection["folder"];
  folderItems: ItemRef[];
  accountItems: ItemRef[];
};

export function matchesSearch(account: PlatformAccount, query: string): boolean {
  return (
    account.id.toLowerCase().includes(query) ||
    account.username.toLowerCase().includes(query) ||
    (account.displayName || "").toLowerCase().includes(query)
  );
}

export function createDisplayPipeline(deps: DisplayPipelineDeps) {
  const { navigation, drag, loader, addFlow, getExpandedFolders, getActiveTab } = deps;

  let isExpandedMode = $derived(
    getExpandedFolders() && navigation.currentFolderId === null && !navigation.isSearching,
  );

  let rawSections = $derived.by<DisplaySection[] | null>(() => {
    if (!isExpandedMode) return null;
    // Subscribe to folder mutations: refreshCurrentItems() reassigns currentItems
    // after any folder/account mutation, which re-fires this derived.
    void navigation.currentItems;
    return getRootSections(getActiveTab());
  });

  let displayFolderItems = $derived.by(() => {
    if (rawSections) return [] as ItemRef[];
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
    if (rawSections) return [] as ItemRef[];
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

  let displaySections = $derived.by<DisplaySection[] | null>(() => {
    if (!rawSections) return null;
    const pending = addFlow.pendingSetupAccount;
    if (!pending) return rawSections;
    const placedSomewhere = rawSections.some((section) =>
      section.accountItems.some((item) => item.id === pending.id),
    );
    if (placedSomewhere) return rawSections;
    const sections = rawSections.map((section) => ({
      folder: section.folder,
      folderItems: section.folderItems,
      accountItems: section.accountItems,
    }));
    const rootIdx = sections.findIndex((s) => s.folder === null);
    if (rootIdx >= 0) {
      sections[rootIdx] = {
        ...sections[rootIdx],
        accountItems: [
          ...sections[rootIdx].accountItems,
          { type: "account" as const, id: pending.id },
        ],
      };
    }
    return sections;
  });

  let visibleRenderedAccountIds = $derived.by(() => {
    const ids: string[] = [];
    const seen = new Set<string>();
    if (displaySections) {
      for (const section of displaySections) {
        for (const item of section.accountItems) {
          if (seen.has(item.id)) continue;
          seen.add(item.id);
          ids.push(item.id);
        }
      }
      return ids;
    }
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
    get displayAccountItemsWithPending() {
      return displayAccountItemsWithPending;
    },
    get displaySections() {
      return displaySections;
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
