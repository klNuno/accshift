import type { MessageKey, TranslationParams } from "$lib/i18n";
import type { AccountWarningChip, AccountWarningPresentation } from "$lib/shared/accountWarnings";
import type { BanInfo } from "./types";

function formatBanTooltip(info: BanInfo): string {
  const lines: string[] = [];
  if (info.community_banned) {
    lines.push("Community ban");
  }
  if (info.vac_banned) {
    const vacCount = Math.max(1, info.number_of_vac_bans || 0);
    lines.push(`${vacCount} VAC ban${vacCount > 1 ? "s" : ""}`);
  }
  if (info.number_of_game_bans > 0) {
    lines.push(`${info.number_of_game_bans} game ban${info.number_of_game_bans > 1 ? "s" : ""}`);
  }
  return lines.join("\n");
}

export function toSteamAccountWarningPresentation(
  banInfo: BanInfo | undefined,
  t: (key: MessageKey, params?: TranslationParams) => string,
): AccountWarningPresentation | undefined {
  if (!banInfo) return undefined;

  const chips: AccountWarningChip[] = [];
  if (banInfo.community_banned) {
    chips.push({ tone: "orange", text: t("ban.community") });
  }
  if (banInfo.vac_banned) {
    const vacCount = Math.max(1, banInfo.number_of_vac_bans || 0);
    chips.push({
      tone: "red",
      text: t(vacCount > 1 ? "ban.vac.multiple" : "ban.vac.single", { count: vacCount }),
    });
  }
  if (banInfo.number_of_game_bans > 0) {
    chips.push({
      tone: "red",
      text: t(
        banInfo.number_of_game_bans > 1 ? "ban.game.multiple" : "ban.game.single",
        { count: banInfo.number_of_game_bans },
      ),
    });
  }

  return {
    tooltipText: formatBanTooltip(banInfo),
    cardOutlineTone: banInfo.vac_banned || banInfo.number_of_game_bans > 0
      ? "red"
      : banInfo.community_banned || (banInfo.economy_ban && banInfo.economy_ban !== "none")
        ? "orange"
        : null,
    listHasRed: banInfo.vac_banned || banInfo.number_of_game_bans > 0,
    listHasOrange: banInfo.community_banned,
    chips,
  };
}
