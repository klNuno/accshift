import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  LOCALES,
  localeFor,
  STREAM_DECK_LOCALES,
  STRINGS,
  streamDeckLocale,
  translate,
} from "../src/i18n.js";
import { ACTION_UUID } from "../src/switch-account.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const pluginDir = join(root, "dev.mtsu.accshift.sdPlugin");
const appLocales = join(root, "..", "..", "crates", "accshift-app", "i18n");

describe("strings", () => {
  test("the seven app languages, each with every English key", () => {
    const app = readdirSync(appLocales)
      .filter((f) => f.endsWith(".json"))
      .map((f) => f.replace(/\.json$/, ""))
      .map((l) => (l === "zh" ? "zh" : l));
    assert.deepEqual([...LOCALES].sort(), app.sort());
    const keys = Object.keys(STRINGS.en).sort();
    for (const locale of LOCALES) {
      assert.deepEqual(Object.keys(STRINGS[locale]).sort(), keys, locale);
      for (const [key, value] of Object.entries(STRINGS[locale])) {
        assert.ok(value.trim(), `${locale} ${key} is empty`);
      }
    }
  });

  test("no em dash or ellipsis character", () => {
    for (const table of Object.values(STRINGS)) {
      for (const value of Object.values(table)) assert.doesNotMatch(value, /[—…]/);
    }
  });

  test("placeholders survive translation", () => {
    for (const locale of LOCALES) {
      assert.match(STRINGS[locale]["pi.signedIn"], /\{name\}/, locale);
      assert.match(STRINGS[locale]["error.failed"], /\{message\}/, locale);
    }
    assert.equal(translate("fr", "pi.signedIn", { name: "Alice" }), "Alice (connecté)");
  });

  test("Stream Deck languages pick a table, English otherwise", () => {
    assert.equal(localeFor("fr"), "fr");
    assert.equal(localeFor("zh_CN"), "zh");
    assert.equal(localeFor("zh_TW"), "zh");
    assert.equal(localeFor("pt-BR"), "pt-br");
    assert.equal(localeFor("de"), "en");
    assert.equal(localeFor(undefined), "en");
    assert.equal(translate("ja", "action.name"), "Switch account");
    assert.equal(translate("en", "no.such.key"), "no.such.key");
  });

  test("the manifest's English text is the table's", () => {
    const manifest = JSON.parse(readFileSync(join(pluginDir, "manifest.json"), "utf8"));
    assert.equal(manifest.Description, STRINGS.en["plugin.description"]);
    const action = manifest.Actions.find((a) => a.UUID === ACTION_UUID);
    assert.ok(action, "the switch action is in the manifest");
    assert.equal(action.Name, STRINGS.en["action.name"]);
    assert.equal(action.Tooltip, STRINGS.en["action.tooltip"]);
    assert.equal(manifest.UUID, "dev.mtsu.accshift");
    assert.equal(manifest.Name, "accshift");
    assert.equal(manifest.Author, "mtsu");
  });

  test("Stream Deck locale files carry the action and the table", () => {
    assert.deepEqual(Object.keys(STREAM_DECK_LOCALES).sort(), ["en", "es", "fr", "zh_CN"]);
    const fr = streamDeckLocale("fr", ACTION_UUID);
    assert.equal(fr.Name, "accshift");
    assert.equal(fr[ACTION_UUID].Name, "Changer de compte");
    assert.equal(fr.Localization["pi.account"], "Compte");
  });

  test("every data-i18n key of the inspector page exists", () => {
    const html = readFileSync(join(pluginDir, "ui", "switch-account.html"), "utf8");
    const keys = [...html.matchAll(/data-i18n="([^"]+)"/g)].map((m) => m[1]);
    assert.ok(keys.length >= 4);
    for (const key of keys) assert.ok(STRINGS.en[key], key);
  });
});
