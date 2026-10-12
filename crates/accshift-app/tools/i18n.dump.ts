// Writes the message dictionaries of src/lib/i18n to crates/accshift-app/i18n,
// one JSON file per locale. The Rust i18n module embeds these files.
//
// From the repo root:
//   build-safe node node_modules/vitest/vitest.mjs run --config crates/accshift-app/tools/vitest.dump.config.mjs
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { LANGUAGE_OPTIONS } from "$lib/i18n";
import { EN_MESSAGES } from "$lib/i18n/messages";
import { ES_MESSAGES } from "$lib/i18n/messages.es";
import { FR_MESSAGES } from "$lib/i18n/messages.fr";
import { PT_MESSAGES } from "$lib/i18n/messages.pt";
import { PT_BR_MESSAGES } from "$lib/i18n/messages.pt-br";
import { RU_MESSAGES } from "$lib/i18n/messages.ru";
import { ZH_MESSAGES } from "$lib/i18n/messages.zh";

const OUT_DIR = fileURLToPath(new URL("../i18n/", import.meta.url));

const DICTIONARIES: Record<string, Record<string, string>> = {
  en: EN_MESSAGES,
  es: ES_MESSAGES,
  fr: FR_MESSAGES,
  pt: PT_MESSAGES,
  "pt-br": PT_BR_MESSAGES,
  ru: RU_MESSAGES,
  zh: ZH_MESSAGES,
};

describe("i18n dump", () => {
  it("writes one dictionary per shipped locale", () => {
    const shipped = LANGUAGE_OPTIONS.map((option) => option.code as string).sort();
    expect(Object.keys(DICTIONARIES).sort()).toEqual(shipped);
    mkdirSync(OUT_DIR, { recursive: true });
    for (const [locale, dictionary] of Object.entries(DICTIONARIES)) {
      writeFileSync(`${OUT_DIR}${locale}.json`, `${JSON.stringify(dictionary, null, 2)}\n`);
    }
  });
});
