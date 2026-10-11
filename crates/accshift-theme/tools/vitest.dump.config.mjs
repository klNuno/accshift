// Vitest config for the variable dump only. The file it runs is named
// `*.dump.ts` so the app's own `vitest run` never picks it up.
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const root = fileURLToPath(new URL("../../..", import.meta.url));

export default defineConfig({
  root,
  resolve: {
    alias: [{ find: "$lib", replacement: `${root}src/lib` }],
  },
  test: {
    include: ["crates/accshift-theme/tools/*.dump.ts"],
    environment: "node",
  },
});
