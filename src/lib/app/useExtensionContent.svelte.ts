import type { CardExtensionContent, CardExtensionSection } from "$lib/shared/cardExtension";
import { warningChipsToExtensionChips } from "$lib/shared/cardExtension";
import type { AccountWarningPresentation } from "$lib/shared/accountWarnings";
import { trackDependencies } from "$lib/shared/trackDependencies";
import type { MessageKey, TranslationParams } from "$lib/i18n";

type Translator = (key: MessageKey, params?: TranslationParams) => string;

type ExtensionContentDeps = {
  t: Translator;
  getLocale: () => string;
  getWarningStates: () => Record<string, AccountWarningPresentation>;
  getVisibleRenderedAccountIds: () => string[];
  getSetupExtensionContent: (accountId: string) => CardExtensionContent | null;
  getAccountNote: (accountId: string) => string;
  getCardNoteVersion: () => number;
  getShowCardNotesInline: () => boolean;
  /** Platform-specific sections appended after warnings/notes (e.g. Steam's
   * CS2 bridge). Version bumps invalidate the memo. */
  getExtraSections?: (accountId: string) => CardExtensionSection[];
  getExtraSectionsVersion?: () => number;
};

export function createExtensionContentController({
  t,
  getLocale,
  getWarningStates,
  getVisibleRenderedAccountIds,
  getSetupExtensionContent,
  getAccountNote,
  getCardNoteVersion,
  getShowCardNotesInline,
  getExtraSections,
  getExtraSectionsVersion,
}: ExtensionContentDeps) {
  function createWarningExtensionSection(
    accountId: string,
  ): CardExtensionContent["sections"][number] | null {
    const warningInfo = getWarningStates()[accountId];
    const warningChips = warningChipsToExtensionChips(warningInfo?.chips);
    const warningLines = warningInfo?.tooltipText
      ? warningInfo.tooltipText
          .split("\n")
          .map((l) => l.trim())
          .filter(Boolean)
      : [];
    if (warningLines.length === 0 && warningChips.length === 0) return null;
    return {
      title: t("card.extensionWarnings"),
      text: warningChips.length > 0 ? undefined : warningLines.join(" \u2022 "),
      lines: warningChips.length > 0 ? [] : warningLines,
      chips: warningChips,
    };
  }

  function createNoteExtensionSection(
    accountId: string,
  ): CardExtensionContent["sections"][number] | null {
    if (getShowCardNotesInline()) return null;
    const note = getAccountNote(accountId).trim();
    if (!note) return null;
    return { title: t("card.extensionNote"), lines: [note] };
  }

  // One entry per account, rebuilt only when that account's own inputs change,
  // so one card's update leaves every other card's prop identity untouched.
  let sharedKey = "";
  let entries = new Map<string, { key: string; content: CardExtensionContent | null }>();
  let extensionCache: Record<string, CardExtensionContent | null> = {};

  function accountKey(id: string): string {
    const w = getWarningStates()[id];
    const setup = getSetupExtensionContent(id);
    return JSON.stringify([
      w?.tooltipText ?? "",
      w?.chips ?? [],
      getAccountNote(id),
      // The full setup content, so flow state changes invalidate the entry.
      setup ? setup.sections : null,
    ]);
  }

  function buildContent(accountId: string): CardExtensionContent | null {
    const setupContent = getSetupExtensionContent(accountId);
    if (setupContent) return setupContent;
    const sections: CardExtensionContent["sections"] = [];
    const warn = createWarningExtensionSection(accountId);
    const note = createNoteExtensionSection(accountId);
    if (warn) sections.push(warn);
    if (note) sections.push(note);
    sections.push(...(getExtraSections?.(accountId) ?? []));
    return sections.length > 0 ? { sections } : null;
  }

  let accountExtensionContentById = $derived.by(() => {
    const locale = getLocale();
    const cardNoteVersion = getCardNoteVersion();
    const showCardNotesInline = getShowCardNotesInline();
    const extraSectionsVersion = getExtraSectionsVersion?.() ?? 0;
    trackDependencies(locale, cardNoteVersion, showCardNotesInline, extraSectionsVersion);
    const ids = getVisibleRenderedAccountIds();

    const nextSharedKey = `${locale}:${cardNoteVersion}:${showCardNotesInline}:${extraSectionsVersion}`;
    const previous = nextSharedKey === sharedKey ? entries : new Map();
    const next = new Map<string, { key: string; content: CardExtensionContent | null }>();
    let changed = previous !== entries || ids.length !== Object.keys(extensionCache).length;
    for (const id of ids) {
      const key = accountKey(id);
      const cached = previous.get(id);
      if (cached && cached.key === key) {
        next.set(id, cached);
        if (!(id in extensionCache)) changed = true;
        continue;
      }
      next.set(id, { key, content: buildContent(id) });
      changed = true;
    }
    sharedKey = nextSharedKey;
    entries = next;
    if (!changed) return extensionCache;

    const map: Record<string, CardExtensionContent | null> = {};
    for (const [id, entry] of next) map[id] = entry.content;
    extensionCache = map;
    return map;
  });

  return {
    get accountExtensionContentById() {
      return accountExtensionContentById;
    },
  };
}
