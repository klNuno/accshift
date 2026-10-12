// Vitest config for the accshift-app dumps only. The files it runs are named
// `*.dump.ts` so the app's own `vitest run` never picks them up.
import { fileURLToPath } from "node:url";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

const root = fileURLToPath(new URL("../../..", import.meta.url));

export default defineConfig({
  root,
  // The i18n runtime is a `.svelte.ts` module ($state runes).
  plugins: [svelte()],
  resolve: {
    alias: [{ find: "$lib", replacement: `${root}src/lib` }],
  },
  test: {
    include: ["crates/accshift-app/tools/*.dump.ts"],
    environment: "node",
  },
});
