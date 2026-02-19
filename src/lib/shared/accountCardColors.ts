const STORAGE_KEY = "accshift_account_card_colors";

export const ACCOUNT_CARD_COLOR_PRESETS = [
  { id: "none", label: "Default", color: "" },
  { id: "slate", label: "Slate", color: "#64748b" },
  { id: "blue", label: "Blue", color: "#3b82f6" },
  { id: "cyan", label: "Cyan", color: "#06b6d4" },
  { id: "emerald", label: "Emerald", color: "#10b981" },
  { id: "amber", label: "Amber", color: "#f59e0b" },
  { id: "rose", label: "Rose", color: "#f43f5e" },
  { id: "violet", label: "Violet", color: "#8b5cf6" },
  { id: "zinc", label: "Graphite", color: "#71717a" },
] as const;

type CardColorMap = Record<string, string>;
let cachedMap: CardColorMap | null = null;
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

function writeMap(data: CardColorMap) {
  cachedMap = data;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
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
