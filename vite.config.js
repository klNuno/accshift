import { fileURLToPath } from "node:url";
import { defineConfig } from "vite-plus";
import tailwindcss from "@tailwindcss/vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Mock mode: every `invoke` resolves to a fake dataset instead of the real
// backend, so nothing on screen is a real account and no click can reach Steam.
// `VITE_MOCK=<scenario>` picks one of src/lib/mock/scenarios.ts; `VITE_DEMO=1`
// is the legacy spelling for the frozen README dataset. Only ever set by hand
// (see scripts/dev-mock.mjs); never in a real build.
const mockScenario = process.env.VITE_DEMO === "1" ? "demo" : (process.env.VITE_MOCK ?? "");
const mockMode = mockScenario !== "";
const mockCore = fileURLToPath(new URL("./src/lib/mock/index.ts", import.meta.url));
// Escape hatch so the mock can re-export the real module (Channel and friends,
// which @tauri-apps/plugin-* import) without the alias below looping back.
const tauriCoreReal = fileURLToPath(
  new URL("./node_modules/@tauri-apps/api/core.js", import.meta.url),
);

// https://vitejs.dev/config/
export default defineConfig({
  test: {},
  // The OFL requires the bundled fonts to ship with their license text
  // unaltered. Oxfmt renumbers its clause list and reflows the paragraphs, so
  // the file is formatted by SIL, not by us.
  fmt: {
    ignorePatterns: ["public/fonts/LICENSE.md"],
  },
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
  // Read by src/lib/mock/runtime.ts. An explicit define rather than
  // import.meta.env: the value has to exist even when the var comes from the
  // shell rather than an .env file.
  define: { __MOCK_SCENARIO__: JSON.stringify(mockScenario) },
  resolve: {
    alias: [
      { find: "$lib", replacement: "/src/lib" },
      // Absolute paths, not "/src/...": the dependency optimizer resolves these
      // from inside node_modules, where a root-relative path does not exist.
      // The exact-match regex keeps the mock's own re-export from looping.
      ...(mockMode
        ? [
            { find: "@tauri-core-real", replacement: tauriCoreReal },
            { find: /^@tauri-apps\/api\/core$/, replacement: mockCore },
          ]
        : []),
    ],
  },
  // The alias points at app source, which the optimizer must not pre-bundle.
  optimizeDeps: mockMode ? { exclude: ["@tauri-apps/api"] } : {},
});
