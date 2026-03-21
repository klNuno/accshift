import type { CardExtensionContent } from "$lib/shared/cardExtension";
import { warningChipsToExtensionChips } from "$lib/shared/cardExtension";
import { trackDependencies } from "$lib/shared/trackDependencies";
import type { MessageKey, TranslationParams } from "$lib/i18n";
import type { AccountWarningPresentation } from "$lib/shared/accountWarnings";

type AddFlowExtensionLike = {
  getSetupExtensionContent: (accountId: string) => CardExtensionContent | null;
  isForcedOpen: (accountId: string) => boolean;
  isPendingSetupAccount: (accountId: string) => boolean;
};

type AccountCardExtensionsOptions = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  getLocale: () => string;
  getCardNoteVersion: () => number;
  getShowCardNotesInline: () => boolean;
  getVisibleRenderedAccountIds: () => string[];
  getWarningStates: () => Record<string, AccountWarningPresentation | undefined>;
  getAccountNote: (accountId: string) => string;
  addFlow: AddFlowExtensionLike;
};

export function createAccountCardExtensionsController({
  t,
  getLocale,
  getCardNoteVersion,
  getShowCardNotesInline,
  getVisibleRenderedAccountIds,
  getWarningStates,
  getAccountNote,
  addFlow,
}: AccountCardExtensionsOptions) {
  function createWarningExtensionSection(
    accountId: string,
  ): CardExtensionContent["sections"][number] | null {
    const warningInfo = getWarningStates()[accountId];
    const warningChips = warningChipsToExtensionChips(warningInfo?.chips);
    const warningLines = warningInfo?.tooltipText
      ? warningInfo.tooltipText
          .split("\n")
          .map((line) => line.trim())
          .filter(Boolean)
      : [];

    if (warningLines.length === 0 && warningChips.length === 0) {
      return null;
    }

    return {
      title: t("card.extensionWarnings"),
      text: warningChips.length > 0 ? undefined : warningLines.join(" • "),
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
    return {
      title: t("card.extensionNote"),
      lines: [note],
    };
  }

  let contentById = $derived.by(() => {
    trackDependencies(getLocale(), getCardNoteVersion(), getShowCardNotesInline());

    const nextContentById: Record<string, CardExtensionContent | null> = {};
    for (const accountId of getVisibleRenderedAccountIds()) {
      const setupContent = addFlow.getSetupExtensionContent(accountId);
      if (setupContent) {
        nextContentById[accountId] = setupContent;
        continue;
      }

      const sections: CardExtensionContent["sections"] = [];
      const warningSection = createWarningExtensionSection(accountId);
      const noteSection = createNoteExtensionSection(accountId);

      if (warningSection) sections.push(warningSection);
      if (noteSection) sections.push(noteSection);

      nextContentById[accountId] = sections.length > 0 ? { sections } : null;
    }

    return nextContentById;
  });

  function isForcedOpen(accountId: string): boolean {
    return addFlow.isForcedOpen(accountId);
  }

  function isPendingSetupAccount(accountId: string): boolean {
    return addFlow.isPendingSetupAccount(accountId);
  }

  return {
    get contentById() {
      return contentById;
    },
    isForcedOpen,
    isPendingSetupAccount,
  };
}
