/**
 * Stands in for `@tauri-apps/api/core` when the dev server runs with
 * `VITE_MOCK=<scenario>` (or the legacy `VITE_DEMO=1`, which selects `demo`).
 *
 * `vite.config.js` aliases the module, so every `invoke` in the app resolves
 * here. Patching `window.__TAURI_INTERNALS__.invoke` at runtime is not an
 * option: Tauri installs it non-writable and non-configurable — hence the
 * build-time alias, which also means none of this can end up in a release.
 *
 * What each piece does: `scenarios.ts` holds the datasets, `fixtures.ts` turns
 * one into a command map, `runtime.ts` dispatches and exposes `window.__mock`.
 */

// Everything the real module exports except `invoke` (the local export below
// wins over `export *`). @tauri-apps/plugin-updater and friends import Channel
// from here, so dropping this would break their module resolution.
export * from "@tauri-core-real";

import { installMockApi, mockInvoke } from "./runtime";

installMockApi();

export function invoke<T>(cmd: string, args?: unknown, options?: unknown): Promise<T> {
  return mockInvoke<T>(cmd, args, options);
}
