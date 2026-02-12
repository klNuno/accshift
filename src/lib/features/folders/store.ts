import type { FolderInfo, ItemRef, FolderStore } from "./types";

const STORE_KEY = "zazaswitcher_folders";

function getStore(): FolderStore {
  try {
    const data = localStorage.getItem(STORE_KEY);
    if (!data) return { folders: [], itemOrder: {} };
    return JSON.parse(data);
  } catch {
    return { folders: [], itemOrder: {} };
  }
}

function saveStore(store: FolderStore) {
  localStorage.setItem(STORE_KEY, JSON.stringify(store));
}

function generateId(): string {
  return Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
}

export function getRootKey(platform: string): string {
  return `root:${platform}`;
}

export function getItemsInFolder(folderId: string | null, platform: string): ItemRef[] {
  const store = getStore();
  const key = folderId || getRootKey(platform);
  return store.itemOrder[key] || [];
}

export function createFolder(name: string, parentId: string | null, platform: string): FolderInfo {
  const store = getStore();
  const folder: FolderInfo = { id: generateId(), name, parentId, platform };
  store.folders.push(folder);

  const parentKey = parentId || getRootKey(platform);
  if (!store.itemOrder[parentKey]) store.itemOrder[parentKey] = [];
  store.itemOrder[parentKey].push({ type: "folder", id: folder.id });

  store.itemOrder[folder.id] = [];

  saveStore(store);
  return folder;
}

export function getFolder(id: string): FolderInfo | undefined {
  return getStore().folders.find(f => f.id === id);
}

export function getFolderPath(folderId: string | null): FolderInfo[] {
  if (!folderId) return [];
  const store = getStore();
  const path: FolderInfo[] = [];
  let currentId: string | null = folderId;
  while (currentId) {
    const folder = store.folders.find(f => f.id === currentId);
    if (!folder) break;
    path.unshift(folder);
    currentId = folder.parentId;
  }
  return path;
}

export function syncAccounts(accountIds: string[], platform: string) {
  const store = getStore();
  const rootKey = getRootKey(platform);
  if (!store.itemOrder[rootKey]) store.itemOrder[rootKey] = [];

  const allPlacedIds = new Set<string>();
  const platformFolderIds = store.folders
    .filter(f => f.platform === platform)
    .map(f => f.id);
  const allKeys = [rootKey, ...platformFolderIds];

  for (const key of allKeys) {
    const items = store.itemOrder[key] || [];
    for (const item of items) {
      if (item.type === "account") allPlacedIds.add(item.id);
    }
  }

  for (const id of accountIds) {
    if (!allPlacedIds.has(id)) {
      store.itemOrder[rootKey].push({ type: "account", id });
    }
  }

  const validIds = new Set(accountIds);
  for (const key of allKeys) {
    if (store.itemOrder[key]) {
      store.itemOrder[key] = store.itemOrder[key].filter(
        item => item.type === "folder" || validIds.has(item.id)
      );
    }
  }

  saveStore(store);
}

export function moveItem(
  itemRef: ItemRef,
  fromFolderId: string | null,
  toFolderId: string | null,
  platform: string,
  insertIndex?: number
) {
  const store = getStore();
  const fromKey = fromFolderId || getRootKey(platform);
  const toKey = toFolderId || getRootKey(platform);

  if (store.itemOrder[fromKey]) {
    store.itemOrder[fromKey] = store.itemOrder[fromKey].filter(
      i => !(i.type === itemRef.type && i.id === itemRef.id)
    );
  }

  if (!store.itemOrder[toKey]) store.itemOrder[toKey] = [];
  if (insertIndex !== undefined) {
    store.itemOrder[toKey].splice(insertIndex, 0, itemRef);
  } else {
    store.itemOrder[toKey].push(itemRef);
  }

  saveStore(store);
}

export function reorderItems(folderId: string | null, platform: string, items: ItemRef[]) {
  const store = getStore();
  const key = folderId || getRootKey(platform);
  store.itemOrder[key] = items;
  saveStore(store);
}

export function deleteFolder(folderId: string) {
  const store = getStore();
  const folder = store.folders.find(f => f.id === folderId);
  if (!folder) return;

  const parentKey = folder.parentId || getRootKey(folder.platform);
  const items = store.itemOrder[folderId] || [];
  if (!store.itemOrder[parentKey]) store.itemOrder[parentKey] = [];

  const parentItems = store.itemOrder[parentKey];
  const folderIdx = parentItems.findIndex(i => i.type === "folder" && i.id === folderId);
  if (folderIdx >= 0) {
    parentItems.splice(folderIdx, 1, ...items);
  } else {
    parentItems.push(...items);
  }

  const subFolders = store.folders.filter(f => f.parentId === folderId);
  for (const sub of subFolders) {
    const subItems = store.itemOrder[sub.id] || [];
    store.itemOrder[parentKey].push(...subItems);
    delete store.itemOrder[sub.id];
    store.folders = store.folders.filter(f => f.id !== sub.id);
  }

  delete store.itemOrder[folderId];
  store.folders = store.folders.filter(f => f.id !== folderId);

  saveStore(store);
}

export function renameFolder(folderId: string, newName: string) {
  const store = getStore();
  const folder = store.folders.find(f => f.id === folderId);
  if (folder) {
    folder.name = newName;
    saveStore(store);
  }
}
