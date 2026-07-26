// Alias declared in vite.config.js under VITE_DEMO=1: the untouched
// @tauri-apps/api/core, reachable from demoCore.ts without the demo alias
// looping back onto itself.
declare module "@tauri-core-real" {
  export * from "@tauri-apps/api/core";
}
