import { fileURLToPath } from "node:url";
import { defineConfig } from "vite-plus";
import tailwindcss from "@tailwindcss/vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Demo recording mode: every `invoke` resolves to a fake dataset instead of the
// real backend, so a capture never shows real accounts and a click can never
// reach Steam. Only ever set by hand when recording; never in a real build.
const demoMode = process.env.VITE_DEMO === "1";
const demoCore = fileURLToPath(new URL("./src/lib/demo/demoCore.ts", import.meta.url));
// Escape hatch so demoCore can re-export the real module (Channel and friends,
// which @tauri-apps/plugin-* import) without the alias below looping back.
const tauriCoreReal = fileURLToPath(
  new URL("./node_modules/@tauri-apps/api/core.js", import.meta.url),
);

// https://vitejs.dev/config/
export default defineConfig({
  test: {},
  staged: {
    "*": "vp check --fix",
  },
  plugins: [tailwindcss(), svelte()],
  build: {
    target: "esnext",
    modulePreload: { polyfill: false },
    reportCompressedSize: false,
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  resolve: {
    alias: [
      { find: "$lib", replacement: "/src/lib" },
      // Absolute paths, not "/src/...": the dependency optimizer resolves these
      // from inside node_modules, where a root-relative path does not exist.
      // The exact-match regex keeps demoCore's own re-export from looping.
      ...(demoMode
        ? [
            { find: "@tauri-core-real", replacement: tauriCoreReal },
            { find: /^@tauri-apps\/api\/core$/, replacement: demoCore },
          ]
        : []),
    ],
  },
  // The alias points at app source, which the optimizer must not pre-bundle.
  optimizeDeps: demoMode ? { exclude: ["@tauri-apps/api"] } : {},
});
