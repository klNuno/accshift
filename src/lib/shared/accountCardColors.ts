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

function readMap(): CardColorMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as CardColorMap;
    if (!parsed || typeof parsed !== "object") return {};
    return parsed;
  } catch {
    return {};
  }
}

function writeMap(data: CardColorMap) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
}

export function getAccountCardColor(accountId: string): string {
  return readMap()[accountId] ?? "";
}

export function setAccountCardColor(accountId: string, color: string) {
  const data = readMap();
  if (!color) {
    delete data[accountId];
  } else {
    data[accountId] = color;
  }
  writeMap(data);
}
