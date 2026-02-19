const STORAGE_KEY = "accshift_folder_card_colors";

type FolderColorMap = Record<string, string>;
let cachedMap: FolderColorMap | null = null;
const SAFE_COLOR_RE = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;

function isSafeColor(color: string): boolean {
  return SAFE_COLOR_RE.test(color);
}

function sanitizeMap(value: unknown): FolderColorMap {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const out: FolderColorMap = {};
  for (const [key, rawColor] of Object.entries(value as Record<string, unknown>)) {
    if (typeof key !== "string" || key.trim().length === 0) continue;
    if (typeof rawColor !== "string") continue;
    if (!isSafeColor(rawColor)) continue;
    out[key] = rawColor;
  }
  return out;
}

function readMap(): FolderColorMap {
  if (cachedMap) return cachedMap;

  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      cachedMap = {};
      return cachedMap;
    }
    cachedMap = sanitizeMap(JSON.parse(raw));
    return cachedMap;
  } catch {
    cachedMap = {};
    return cachedMap;
  }
}

function writeMap(data: FolderColorMap) {
  cachedMap = data;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
}

export function getFolderCardColor(folderId: string): string {
  return readMap()[folderId] ?? "";
}

export function setFolderCardColor(folderId: string, color: string) {
  const data = readMap();
  if (!color || !isSafeColor(color)) {
    delete data[folderId];
  } else {
    data[folderId] = color;
  }
  writeMap(data);
}
