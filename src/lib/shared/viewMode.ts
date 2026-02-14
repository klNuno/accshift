export type ViewMode = "grid" | "list";

const STORAGE_KEY = "zazaswitcher_viewMode";

export function getViewMode(): ViewMode {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "list") return "list";
  return "grid";
}

export function setViewMode(mode: ViewMode): void {
  localStorage.setItem(STORAGE_KEY, mode);
}
