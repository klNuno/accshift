import {
  CLIENT_STORE_VIEW_MODE,
  getClientStoreValue,
  setClientStoreValue,
} from "$lib/storage/clientStorage";

export type ViewMode = "grid" | "list";

export function getViewMode(): ViewMode {
  const stored = getClientStoreValue<string>(CLIENT_STORE_VIEW_MODE);
  if (stored === "list") return "list";
  return "grid";
}

export function setViewMode(mode: ViewMode): void {
  setClientStoreValue(CLIENT_STORE_VIEW_MODE, mode);
}
