import {
  CLIENT_STORE_ACCOUNT_CARD_COLORS,
  getClientStoreRevision,
  getClientStoreValue,
  setClientStoreValue,
} from "$lib/storage/clientStorage";

export const ACCOUNT_CARD_COLOR_PRESETS = [
  { id: "none", color: "" },
  { id: "blue", color: "#3b82f6" },
  { id: "cyan", color: "#06b6d4" },
  { id: "green", color: "#10b981" },
  { id: "lime", color: "#84cc16" },
  { id: "yellow", color: "#f59e0b" },
  { id: "orange", color: "#f97316" },
  { id: "red", color: "#f43f5e" },
  { id: "pink", color: "#ec4899" },
  { id: "violet", color: "#8b5cf6" },
  { id: "gray", color: "#71717a" },
] as const;

type CardColorMap = Record<string, string>;
let cachedMap: CardColorMap | null = null;
let cachedRevision = -1;
const SAFE_COLOR_RE = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;

function isSafeColor(color: string): boolean {
  return SAFE_COLOR_RE.test(color);
}

function sanitizeMap(value: unknown): CardColorMap {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const out: CardColorMap = {};
  for (const [key, rawColor] of Object.entries(value as Record<string, unknown>)) {
    if (typeof key !== "string" || key.trim().length === 0) continue;
    if (typeof rawColor !== "string") continue;
    if (!isSafeColor(rawColor)) continue;
    out[key] = rawColor;
  }
  return out;
}

function readMap(): CardColorMap {
  const revision = getClientStoreRevision(CLIENT_STORE_ACCOUNT_CARD_COLORS);
  if (cachedMap && cachedRevision === revision) return cachedMap;

  try {
    const raw = getClientStoreValue<unknown>(CLIENT_STORE_ACCOUNT_CARD_COLORS);
    if (raw == null) {
      cachedMap = {};
      cachedRevision = revision;
      return cachedMap;
    }
    cachedMap = sanitizeMap(raw);
    cachedRevision = revision;
    return cachedMap;
  } catch {
    cachedMap = {};
    cachedRevision = revision;
    return cachedMap;
  }
}

function writeMap(data: CardColorMap) {
  cachedMap = data;
  setClientStoreValue(CLIENT_STORE_ACCOUNT_CARD_COLORS, data);
  cachedRevision = getClientStoreRevision(CLIENT_STORE_ACCOUNT_CARD_COLORS);
}

export function getAccountCardColor(accountId: string): string {
  return readMap()[accountId] ?? "";
}

export function setAccountCardColor(accountId: string, color: string) {
  const data = readMap();
  if (!color || !isSafeColor(color)) {
    delete data[accountId];
  } else {
    data[accountId] = color;
  }
  writeMap(data);
}
