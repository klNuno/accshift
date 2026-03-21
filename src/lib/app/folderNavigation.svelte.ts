import { getItemsInFolder, getFolderPath } from "$lib/features/folders/store";
import type { ItemRef } from "$lib/features/folders/types";

export type AppHistoryEntry = {
  tab: string;
  folderId: string | null;
  showSettings: boolean;
};

export function createFolderNavigation(getActiveTab: () => string) {
  let currentFolderId = $state<string | null>(null);
  let currentItems = $state<ItemRef[]>([]);
  let folderPath = $derived(getFolderPath(currentFolderId));
  let folderItems = $derived(currentItems.filter((item) => item.type === "folder"));
  let accountItems = $derived(currentItems.filter((item) => item.type === "account"));
  let searchQuery = $state("");
  let isSearching = $derived(searchQuery.trim().length > 0);

  function refreshCurrentItems() {
    currentItems = getItemsInFolder(currentFolderId, getActiveTab());
  }

  return {
    get currentFolderId() {
      return currentFolderId;
    },
    set currentFolderId(next: string | null) {
      currentFolderId = next;
    },
    get currentItems() {
      return currentItems;
    },
    set currentItems(next: ItemRef[]) {
      currentItems = next;
    },
    get folderPath() {
      return folderPath;
    },
    get folderItems() {
      return folderItems;
    },
    get accountItems() {
      return accountItems;
    },
    get searchQuery() {
      return searchQuery;
    },
    set searchQuery(next: string) {
      searchQuery = next;
    },
    get isSearching() {
      return isSearching;
    },
    refreshCurrentItems,
  };
}
