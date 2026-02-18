const STORAGE_KEY = "accshift_folder_card_colors";

type FolderColorMap = Record<string, string>;

function readMap(): FolderColorMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as FolderColorMap;
    if (!parsed || typeof parsed !== "object") return {};
    return parsed;
  } catch {
    return {};
  }
}

function writeMap(data: FolderColorMap) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
}

export function getFolderCardColor(folderId: string): string {
  return readMap()[folderId] ?? "";
}

export function setFolderCardColor(folderId: string, color: string) {
  const data = readMap();
  if (!color) {
    delete data[folderId];
  } else {
    data[folderId] = color;
  }
  writeMap(data);
}
