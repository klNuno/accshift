import type { PlatformAccount } from "$lib/shared/platform";
import type { ItemRef } from "$lib/features/folders/types";
import { getFolder, getItemsInFolder, getRootSections } from "$lib/features/folders/store";
import type { DisplaySection } from "$lib/shared/sections";
import { getAccountCardNote } from "$lib/shared/accountCardNotes";

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

// Moved to the shared layer so shared components can use it without
// depending on app code; re-exported here for existing app-layer imports.
export type { DisplaySection };

const DIACRITICS_RE = /[\u0300-\u036f]/g;

// Accent-insensitive, case-insensitive comparison base.
export function foldSearchText(value: string): string {
  return value.normalize("NFD").replace(DIACRITICS_RE, "").toLowerCase();
}

export function matchesSearch(account: PlatformAccount, query: string): boolean {
  const q = foldSearchText(query);
  return (
    foldSearchText(account.id).includes(q) ||
    foldSearchText(account.username).includes(q) ||
    foldSearchText(account.displayName || "").includes(q) ||
    foldSearchText(getAccountCardNote(account.id)).includes(q)
  );
}

// Walk the platform's folder tree and keep the folders whose name matches.
function findMatchingFolderItems(platform: string, foldedQuery: string): ItemRef[] {
  const out: ItemRef[] = [];
  const seen = new Set<string>();
  const queue = getItemsInFolder(null, platform).filter((item) => item.type === "folder");
  for (let i = 0; i < queue.length; i++) {
    const ref = queue[i];
    if (seen.has(ref.id)) continue;
    seen.add(ref.id);
    const folder = getFolder(ref.id);
    if (!folder) continue;
    for (const child of getItemsInFolder(folder.id, platform)) {
      if (child.type === "folder") queue.push(child);
    }
    if (foldSearchText(folder.name).includes(foldedQuery)) {
      out.push({ type: "folder", id: folder.id });
    }
  }
  return out;
}

const SEARCH_DEBOUNCE_MS = 80;

export function createDisplayPipeline(deps: DisplayPipelineDeps) {
  const { navigation, drag, loader, addFlow, getExpandedFolders, getActiveTab } = deps;

  // Each keystroke re-filters every account and re-renders the grid; debounce
  // so a fast typist pays once, not per character. Clearing stays instant.
  let debouncedSearchQuery = $state("");
  $effect(() => {
    const raw = navigation.searchQuery;
    if (raw.trim() === "") {
      debouncedSearchQuery = "";
      return;
    }
    const timer = setTimeout(() => {
      debouncedSearchQuery = raw;
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

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
    if (navigation.isSearching) {
      // Folders whose name matches the query stay reachable during a search.
      // Subscribe to currentItems so folder mutations re-fire this derived.
      void navigation.currentItems;
      const q = debouncedSearchQuery.trim();
      if (!q) return [] as ItemRef[];
      return findMatchingFolderItems(getActiveTab(), foldSearchText(q));
    }
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
    const q = debouncedSearchQuery.trim().toLowerCase();
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

  // A drag only churns the preview order, never the set of visible accounts.
  // Early-out on drag and keep the previous array identity when the contents
  // did not change, so O(N) consumers (avatar priming, etc.) stay idle.
  let lastVisibleIds: string[] = [];
  let visibleRenderedAccountIds = $derived.by(() => {
    if (drag.isDragging && lastVisibleIds.length > 0) return lastVisibleIds;
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
    } else {
      for (const item of displayAccountItemsWithPending) {
        if (item.type !== "account" || seen.has(item.id)) continue;
        seen.add(item.id);
        ids.push(item.id);
      }
    }
    if (
      ids.length !== lastVisibleIds.length ||
      ids.some((id, index) => id !== lastVisibleIds[index])
    ) {
      lastVisibleIds = ids;
    }
    return lastVisibleIds;
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
