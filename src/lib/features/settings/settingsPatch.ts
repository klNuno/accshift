import type { AppSettings } from "./types";

/** One setting that changed: where it sits and its new value. */
export interface SettingsChange {
  path: string[];
  value: unknown;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function cloneJson<T>(value: T): T {
  return value === undefined ? value : (JSON.parse(JSON.stringify(value)) as T);
}

/**
 * Every leaf that differs between `before` and `after`. Objects are walked
 * key by key; lists and scalars are compared and replaced whole.
 */
export function diffSettings(before: unknown, after: unknown, path: string[] = []): SettingsChange[] {
  if (isPlainObject(before) && isPlainObject(after)) {
    const keys = new Set([...Object.keys(before), ...Object.keys(after)]);
    return [...keys].flatMap((key) => diffSettings(before[key], after[key], [...path, key]));
  }
  return JSON.stringify(before) === JSON.stringify(after) ? [] : [{ path, value: after }];
}

function applyChange(target: Record<string, unknown>, change: SettingsChange) {
  let node = target;
  for (const key of change.path.slice(0, -1)) {
    if (!isPlainObject(node[key])) node[key] = {};
    node = node[key] as Record<string, unknown>;
  }
  const leaf = change.path[change.path.length - 1];
  if (change.value === undefined) {
    delete node[leaf];
  } else {
    node[leaf] = cloneJson(change.value);
  }
}

/**
 * The settings to save from an open panel: `current` (the store as it is now)
 * with only the fields the user changed since `baseline` taken from `draft`.
 *
 * The panel holds a copy taken when it opened. Saving that copy whole would
 * put back every field something else wrote meanwhile: the zoom shortcuts,
 * the streamer banner, a PIN hash upgraded on unlock.
 */
export function mergeSettingsDraft(
  current: AppSettings,
  baseline: AppSettings,
  draft: AppSettings,
): AppSettings {
  const merged = cloneJson(current) as unknown as Record<string, unknown>;
  for (const change of diffSettings(baseline, draft)) {
    if (change.path.length > 0) applyChange(merged, change);
  }
  return merged as unknown as AppSettings;
}
