import { describe, it, expect } from "vitest";
import { EN_MESSAGES } from "./messages";
import { ES_MESSAGES } from "./messages.es";
import { FR_MESSAGES } from "./messages.fr";
import { PT_MESSAGES } from "./messages.pt";
import { PT_BR_MESSAGES } from "./messages.pt-br";
import { RU_MESSAGES } from "./messages.ru";
import { ZH_MESSAGES } from "./messages.zh";
import { LANGUAGE_OPTIONS, type Locale } from "./index";

type Dictionary = Record<string, string>;

// Every locale except the bundled English default. Adding a locale means adding
// it here too: the "dictionary for every shipped locale" test below fails
// otherwise, so this map cannot silently drift from LANGUAGE_OPTIONS.
const TRANSLATIONS: Record<Exclude<Locale, "en">, Dictionary> = {
  es: ES_MESSAGES,
  fr: FR_MESSAGES,
  pt: PT_MESSAGES,
  "pt-br": PT_BR_MESSAGES,
  ru: RU_MESSAGES,
  zh: ZH_MESSAGES,
};

const ALL_DICTIONARIES: Array<[string, Dictionary]> = [
  ["en", EN_MESSAGES],
  ...Object.entries(TRANSLATIONS),
];

const EN_KEYS = Object.keys(EN_MESSAGES).sort();

// "{platform} is restarting" -> ["{platform}"]. Interpolation is name based, so
// a translation may reorder placeholders, but it may never drop, invent or
// misspell one: translate() would then render an empty string or a raw brace.
function placeholders(value: string): string[] {
  return (value.match(/\{\w+\}/g) ?? []).sort();
}

describe("message dictionaries", () => {
  it("has a dictionary for every shipped locale", () => {
    const shipped = LANGUAGE_OPTIONS.map((option) => option.code as string).sort();
    const registered = ["en", ...Object.keys(TRANSLATIONS)].sort();
    expect(registered).toEqual(shipped);
  });

  describe.each(Object.entries(TRANSLATIONS))("%s", (locale, dictionary) => {
    it("covers exactly the en key set", () => {
      expect(Object.keys(dictionary).sort()).toEqual(EN_KEYS);
    });

    it("keeps the en placeholders of every string", () => {
      for (const key of EN_KEYS) {
        expect(placeholders(dictionary[key] ?? ""), `placeholders in ${locale}:${key}`).toEqual(
          placeholders(EN_MESSAGES[key as keyof typeof EN_MESSAGES]),
        );
      }
    });
  });

  it.each(ALL_DICTIONARIES)("%s has no em or en dash in any UI string", (_locale, dictionary) => {
    for (const [key, value] of Object.entries(dictionary)) {
      expect(value, `dash in ${key}`).not.toMatch(/[–—]/);
    }
  });

  it.each(ALL_DICTIONARIES)("%s has no empty string", (_locale, dictionary) => {
    for (const [key, value] of Object.entries(dictionary)) {
      expect(value.trim(), `empty message for ${key}`).not.toBe("");
    }
  });
});
