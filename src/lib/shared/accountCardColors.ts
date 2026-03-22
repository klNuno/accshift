import { CLIENT_STORE_ACCOUNT_CARD_COLORS } from "$lib/storage/clientStorage";
import { createCachedMapStore } from "./cachedMapStore";

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

const SAFE_COLOR_RE = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;

const store = createCachedMapStore(CLIENT_STORE_ACCOUNT_CARD_COLORS, (_key, color) =>
  SAFE_COLOR_RE.test(color) ? color : null,
);

export const getAccountCardColor = store.get;
export const setAccountCardColor = store.set;
