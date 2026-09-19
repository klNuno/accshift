/**
 * Which dictionary the next start fetches together with the bundle.
 *
 * main.ts only asked for the saved language's dictionary once the whole bundle
 * had run, and mounting waits for it: one more request to the asset protocol
 * in the middle of the boot. The head script vite.config.js writes into
 * index.html reads this key and imports the chunk of that language while the
 * bundle downloads, then leaves the dictionary for loadLocaleMessages().
 */

// Read by the head script under the same name.
const BOOT_LOCALE_KEY = "accshift_boot_locale";

export function rememberBootLocale(locale: string): void {
  try {
    // English ships in the bundle: the head finds no chunk for it.
    if (localStorage.getItem(BOOT_LOCALE_KEY) !== locale) {
      localStorage.setItem(BOOT_LOCALE_KEY, locale);
    }
  } catch {
    // Storage off or full: the next start fetches the dictionary after the
    // bundle, as it always did.
  }
}
