import { defineConfig } from "vite-plus";
import tailwindcss from "@tailwindcss/vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// https://vitejs.dev/config/
export default defineConfig({
  test: {
    passWithNoTests: true,
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
  resolve: {
    alias: {
      $lib: "/src/lib",
    },
  },
});
