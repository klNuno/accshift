import type { FolderInfo, ItemRef, FolderStore } from "./types";

const STORE_KEY = "accshift_folders";
const CURRENT_VERSION = 1;
let cachedStore: FolderStore | null = null;

function asRecord(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  return value as Record<string, unknown>;
}

function sanitizeFolder(value: unknown): FolderInfo | null {
  const raw = asRecord(value);
  const id = typeof raw.id === "string" ? raw.id.trim() : "";
  const name = typeof raw.name === "string" ? raw.name.trim() : "";
  const platform = typeof raw.platform === "string" ? raw.platform.trim() : "";
  const parentIdRaw = raw.parentId;
  const parentId = parentIdRaw === null
    ? null
    : typeof parentIdRaw === "string" && parentIdRaw.trim().length > 0
      ? parentIdRaw.trim()
      : null;

  if (!id || !name || !platform) return null;

  return { id, name, parentId, platform };
}

function sanitizeItemRef(value: unknown): ItemRef | null {
  const raw = asRecord(value);
  const type = raw.type;
  const id = typeof raw.id === "string" ? raw.id.trim() : "";
  if (!id) return null;
  if (type !== "account" && type !== "folder") return null;
  return { type, id };
}

function sanitizeStore(value: unknown): FolderStore {
  const raw = asRecord(value);
  const foldersRaw = Array.isArray(raw.folders) ? raw.folders : [];
  const folders = foldersRaw
    .map(sanitizeFolder)
    .filter((folder): folder is FolderInfo => folder !== null);
  const validFolderIds = new Set(folders.map((folder) => folder.id));
  const itemOrder: Record<string, ItemRef[]> = {};
  const itemOrderRaw = asRecord(raw.itemOrder);

  for (const [key, entry] of Object.entries(itemOrderRaw)) {
    if (typeof key !== "string" || key.trim().length === 0) continue;
    if (!Array.isArray(entry)) continue;
    const refs = entry
      .map(sanitizeItemRef)
      .filter((item): item is ItemRef => item !== null)
      .filter((item) => item.type !== "folder" || validFolderIds.has(item.id));
    itemOrder[key] = refs;
  }

  return {
    version: CURRENT_VERSION,
    folders,
    itemOrder,
  };
}

function migrateStore(store: FolderStore): FolderStore {
  if (!store.version) {
    // Upgrade legacy payloads that did not store a schema version.
    store.version = CURRENT_VERSION;
  }
  return store;
}

function getStore(): FolderStore {
  if (cachedStore) return cachedStore;

  try {
    const data = localStorage.getItem(STORE_KEY);
    if (!data) {
      cachedStore = { version: CURRENT_VERSION, folders: [], itemOrder: {} };
      return cachedStore;
    }
    const store = sanitizeStore(JSON.parse(data));
    cachedStore = migrateStore(store);
    return cachedStore;
  } catch {
    cachedStore = { version: CURRENT_VERSION, folders: [], itemOrder: {} };
    return cachedStore;
  }
}

function saveStore(store: FolderStore) {
  store.version = CURRENT_VERSION;
  cachedStore = store;
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
