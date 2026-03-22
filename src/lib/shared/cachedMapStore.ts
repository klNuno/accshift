import {
  type ClientStoreId,
  getClientStoreRevision,
  getClientStoreValue,
  setClientStoreValue,
} from "$lib/storage/clientStorage";

type StringMap = Record<string, string>;

export function createCachedMapStore(
  storeId: ClientStoreId,
  sanitizeEntry: (key: string, value: string) => string | null,
) {
  let cachedMap: StringMap | null = null;
  let cachedRevision = -1;

  function sanitizeMap(value: unknown): StringMap {
    if (!value || typeof value !== "object" || Array.isArray(value)) return {};
    const out: StringMap = {};
    for (const [key, raw] of Object.entries(value as Record<string, unknown>)) {
      if (typeof key !== "string" || key.trim().length === 0) continue;
      if (typeof raw !== "string") continue;
      const safe = sanitizeEntry(key, raw);
      if (safe !== null) out[key] = safe;
    }
    return out;
  }

  function readMap(): StringMap {
    const revision = getClientStoreRevision(storeId);
    if (cachedMap && cachedRevision === revision) return cachedMap;
    try {
      const raw = getClientStoreValue<unknown>(storeId);
      cachedMap = raw == null ? {} : sanitizeMap(raw);
    } catch {
      cachedMap = {};
    }
    cachedRevision = revision;
    return cachedMap;
  }

  function writeMap(data: StringMap) {
    cachedMap = data;
    setClientStoreValue(storeId, data);
    cachedRevision = getClientStoreRevision(storeId);
  }

  return {
    get(id: string): string {
      return readMap()[id] ?? "";
    },
    set(id: string, value: string) {
      const data = readMap();
      const safe = sanitizeEntry(id, value);
      if (safe === null) {
        delete data[id];
      } else {
        data[id] = safe;
      }
      writeMap(data);
    },
    remove(id: string) {
      const data = readMap();
      delete data[id];
      writeMap(data);
    },
  };
}
