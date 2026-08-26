import type { CardExtensionSection } from "$lib/shared/cardExtension";
import type { MessageKey } from "$lib/i18n";
import { getCs2BridgeData } from "./cs2Bridge.svelte";

type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

/**
 * The CS2 level, XP bar and weekly-case chip an account card shows when the
 * CS2 bridge has data for it.
 *
 * Empty until the bridge has both a level and an XP reading: a partial row
 * would render a progress bar with nothing behind it.
 */
export function cs2ExtensionSections(accountId: string, t: Translate): CardExtensionSection[] {
  const data = getCs2BridgeData(accountId);
  if (!data || data.level === null || data.xp === null) return [];
  return [
    {
      title: t("card.cs2Section"),
      text: t("card.cs2Level", { level: data.level }),
      progress: {
        value: data.xp,
        max: data.xpMax,
        label: `${data.xp}/${data.xpMax}`,
      },
      chips: [
        data.caseEarned
          ? { text: t("card.cs2CaseEarned"), tone: "green" as const }
          : { text: t("card.cs2CaseNotEarned"), tone: "slate" as const },
      ],
    },
  ];
}

/** The same weekly-case state, as the badge shown next to the username. */
export function cs2UsernameBadge(accountId: string, t: Translate) {
  const data = getCs2BridgeData(accountId);
  if (!data) return null;
  return data.caseEarned
    ? { tone: "green" as const, label: t("card.cs2CaseEarned") }
    : { tone: "slate" as const, label: t("card.cs2CaseNotEarned") };
}
