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

// main.ts waits for the boot payload before anything else, and main.ts only
// runs once every chunk of the bundle has been fetched and evaluated. This asks
// for the payload from the document head instead, while those chunks are still
// on their way; fetchBootPayload() picks up the promise. Left out of mock
// builds: it calls the backend directly, past the mock alias, and
// get_boot_payload runs the legacy config migration.
function bootPayloadPrefetch() {
  return {
    name: "accshift-boot-payload-prefetch",
    transformIndexHtml() {
      if (mockMode) return [];
      return [
        {
          tag: "script",
          injectTo: "head-prepend",
          children:
            'window.__accshiftBootPayload = window.__TAURI_INTERNALS__?.invoke("get_boot_payload");' +
            // Handled for real in fetchBootPayload; this only keeps an early
            // failure from reporting as an unhandled rejection meanwhile.
            "window.__accshiftBootPayload?.catch(() => {});",
        },
      ];
    },
  };
}

// main.ts waits for the saved language's dictionary before mounting, and that
// chunk was only asked for once the whole bundle had run. The head now imports
// the one the last start used (src/lib/app/bootLocale.ts leaves its code in
// localStorage): it downloads alongside the bundle and runs while the page
// still waits for the bundle, and leaves the dictionary where
// loadLocaleMessages() takes it without an import of its own. Safe to run that
// early because a dictionary chunk imports nothing; its one export is the
// dictionary.
// First thing in the head, ahead of the boot payload request: the first invoke
// of a page spends ~4.5 ms setting up window.chrome.webview, and the dictionary
// request waited behind it, one round trip after the bundle's. Also well before
// the stylesheet link: an inline script placed after that link waits for the
// stylesheet. Build only; the dev server has no chunks.
function localePreload() {
  const anchor = "<head>";
  const charset = '<meta charset="UTF-8" />';
  return {
    name: "accshift-locale-preload",
    transformIndexHtml: {
      order: "post",
      handler(html, ctx) {
        if (!ctx.bundle) return html;
        if (!html.includes(anchor)) throw new Error(`index.html lost its ${anchor}`);
        const chunks = {};
        for (const chunk of Object.values(ctx.bundle)) {
          const locale =
            chunk.type === "chunk" &&
            /\/i18n\/messages\.([a-z-]+)\.ts$/.exec(chunk.facadeModuleId ?? "")?.[1];
          if (!locale) continue;
          if (chunk.imports.length > 0 || chunk.dynamicImports.length > 0) {
            throw new Error(
              `${chunk.fileName} imports other chunks; the head would run them early`,
            );
          }
          if (chunk.exports.length !== 1) {
            throw new Error(`${chunk.fileName} must export its dictionary and nothing else`);
          }
          chunks[locale] = `/${chunk.fileName}`;
        }
        // Caught so that a failure is reported once, by the import in main.ts,
        // which meets the same one.
        const script =
          `<script>try{const locale=localStorage.getItem("accshift_boot_locale"),href=${JSON.stringify(chunks)}[locale];` +
          "if(href)import(href).then((m)=>{window.__accshiftBootDictionary={locale,dictionary:Object.values(m)[0]}}).catch(()=>{})}catch{}</script>";
        const out = html.replace(anchor, `${anchor}\n    ${script}`);
        // The charset has to be declared in the first 1024 bytes, and it now
        // comes after this script.
        const charsetEnd = out.indexOf(charset) + charset.length;
        if (charsetEnd < charset.length || Buffer.byteLength(out.slice(0, charsetEnd)) > 1024) {
          throw new Error(`index.html declares its charset past the first 1024 bytes`);
        }
        return out;
      },
    },
  };
}

// https://vitejs.dev/config/
export default defineConfig({
  test: {},
  // The OFL requires the bundled fonts to ship with their license text
  // unaltered. Oxfmt renumbers its clause list and reflows the paragraphs, so
  // the file is formatted by SIL, not by us. vendor/ is upstream code, kept as
  // published so a later update diffs cleanly.
  fmt: {
    ignorePatterns: ["public/fonts/LICENSE.md", "vendor/**"],
  },
  staged: {
    "*": "vp check --fix",
  },
  plugins: [tailwindcss(), svelte(), bootPayloadPrefetch(), localePreload()],
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
