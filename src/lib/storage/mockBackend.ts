/** Injected by vite.config.js from VITE_MOCK / VITE_DEMO; empty in a real build. */
declare const __MOCK_SCENARIO__: string | undefined;

/**
 * True when every `invoke` goes to the mock backend (`pnpm dev:mock`). The
 * define is replaced at build time, so a real build folds this to false.
 */
export function isMockBackend(): boolean {
  return typeof __MOCK_SCENARIO__ === "string" && __MOCK_SCENARIO__ !== "";
}
