import type { CardExtensionContent } from "$lib/shared/cardExtension";
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

  let extensionCacheKey = "";
  let extensionCache: Record<string, CardExtensionContent | null> = {};

  let accountExtensionContentById = $derived.by(() => {
    const locale = getLocale();
    const cardNoteVersion = getCardNoteVersion();
    const showCardNotesInline = getShowCardNotesInline();
    trackDependencies(locale, cardNoteVersion, showCardNotesInline);
    const ids = getVisibleRenderedAccountIds();
    // Build a key that captures all inputs per account
    const keyParts: string[] = [];
    for (const id of ids) {
      const w = getWarningStates()[id];
      const n = getAccountNote(id);
      const s = getSetupExtensionContent(id) ? "s" : "";
      keyParts.push(`${id}:${w?.tooltipText ?? ""}:${w?.chips?.length ?? 0}:${n}:${s}`);
    }
    const newKey = `${locale}:${cardNoteVersion}:${showCardNotesInline}:${keyParts.join("|")}`;
    if (newKey === extensionCacheKey) return extensionCache;

    const map: Record<string, CardExtensionContent | null> = {};
    for (const accountId of ids) {
      const setupContent = getSetupExtensionContent(accountId);
      if (setupContent) {
        map[accountId] = setupContent;
        continue;
      }
      const sections: CardExtensionContent["sections"] = [];
      const warn = createWarningExtensionSection(accountId);
      const note = createNoteExtensionSection(accountId);
      if (warn) sections.push(warn);
      if (note) sections.push(note);
      map[accountId] = sections.length > 0 ? { sections } : null;
    }
    extensionCacheKey = newKey;
    extensionCache = map;
    return map;
  });

  return {
    get accountExtensionContentById() {
      return accountExtensionContentById;
    },
  };
}
