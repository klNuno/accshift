export function formatRelativeTimeFromUnixSeconds(timestamp?: number | null): string {
  if (!timestamp || !Number.isFinite(timestamp) || timestamp <= 0) return "unknown";
  const thenMs = timestamp * 1000;
  const nowMs = Date.now();
  const deltaMs = Math.max(0, nowMs - thenMs);
  const minute = 60 * 1000;
  const hour = 60 * minute;
  const day = 24 * hour;

  if (deltaMs < minute) return "just now";
  if (deltaMs < hour) {
    const m = Math.floor(deltaMs / minute);
    return `${m} minute${m > 1 ? "s" : ""} ago`;
  }
  if (deltaMs < day) {
    const h = Math.floor(deltaMs / hour);
    return `${h} hour${h > 1 ? "s" : ""} ago`;
  }
  const d = Math.floor(deltaMs / day);
  return `${d} day${d > 1 ? "s" : ""} ago`;
}

export function formatRelativeTimeCompact(timestamp?: number | null): string {
  const base = formatRelativeTimeFromUnixSeconds(timestamp);
  return `(${base})`;
}
