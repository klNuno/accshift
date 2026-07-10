import { EN_MESSAGES, type MessageKey } from "./messages";

export type Locale = "en" | "fr";

type Dictionary = Record<MessageKey, string>;

// English ships in the main chunk: it is both the default locale and the
// fallback for keys of a locale that has not finished loading. Every other
// locale is pulled in on demand through a dynamic import so its dictionary
// stays out of the initial bundle.
const LOCALE_LOADERS: Partial<Record<Locale, () => Promise<Dictionary>>> = {
  fr: () => import("./messages.fr").then((module) => module.FR_MESSAGES),
};

// $state.raw: the record is replaced wholesale when a locale finishes
// loading, which re-runs every template that read a dictionary through
// translate() and swaps the EN fallback for the real strings in one commit.
let dictionaries = $state.raw<Partial<Record<Locale, Dictionary>>>({ en: EN_MESSAGES });

const pendingLoads = new Map<Locale, Promise<void>>();

export function getDictionary(locale: Locale): Dictionary | undefined {
  return dictionaries[locale];
}

/**
 * Loads the message dictionary for `locale` (no-op for bundled or already
 * loaded locales). A failed load rejects but stays retryable: translate()
 * falls back to English in the meantime and re-triggers the load on the next
 * call for that locale.
 */
export function loadLocaleMessages(locale: Locale): Promise<void> {
  if (dictionaries[locale]) return Promise.resolve();
  const loader = LOCALE_LOADERS[locale];
  if (!loader) return Promise.resolve();

  let pending = pendingLoads.get(locale);
  if (!pending) {
    pending = loader()
      .then((dictionary) => {
        dictionaries = { ...dictionaries, [locale]: dictionary };
      })
      .finally(() => {
        pendingLoads.delete(locale);
      });
    pendingLoads.set(locale, pending);
  }
  return pending;
}
