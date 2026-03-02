import { addToast, removeToast } from "$lib/features/notifications/store.svelte";
import { getSettings } from "$lib/features/settings/store";
import type { AccountWarningChip, AccountWarningPresentation } from "$lib/shared/accountWarnings";
import type { PlatformAccount, PlatformUiCallbacks, PlatformWarningLoadOptions } from "$lib/shared/platform";
import type { MessageKey, TranslationParams } from "$lib/i18n";
import { getPlayerBans, hasApiKey } from "./steamApi";
import type { BanInfo } from "./types";

const BAN_CHECK_STATE_KEY = "accshift_ban_check_state_v2";
const BAN_INFO_CACHE_KEY = "accshift_ban_info_cache_v1";
const BAN_ERROR_TOAST_COOLDOWN_MS = 30000;

interface BanCheckState {
  lastSuccessAt: number;
  checkedSteamIds: string[];
}

let sessionBanCheckedIds = new Set<string>();
let lastBanErrorToastAt = 0;
let activeBanCheckToastId: string | null = null;

function readBanCheckState(): BanCheckState | null {
  try {
    const raw = localStorage.getItem(BAN_CHECK_STATE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<BanCheckState>;
    const lastSuccessAt = Number(parsed.lastSuccessAt);
    const checkedSteamIds = Array.isArray(parsed.checkedSteamIds)
      ? parsed.checkedSteamIds.filter((id): id is string => typeof id === "string")
      : [];
    if (!Number.isFinite(lastSuccessAt) || lastSuccessAt < 0) return null;
    return {
      lastSuccessAt,
      checkedSteamIds: Array.from(new Set(checkedSteamIds)),
    };
  } catch {
    return null;
  }
}

function writeBanCheckState(state: BanCheckState) {
  localStorage.setItem(BAN_CHECK_STATE_KEY, JSON.stringify(state));
}

function isBanInfo(value: unknown): value is BanInfo {
  if (!value || typeof value !== "object") return false;
  const v = value as Partial<BanInfo>;
  return typeof v.steam_id === "string"
    && typeof v.community_banned === "boolean"
    && typeof v.vac_banned === "boolean"
    && typeof v.number_of_vac_bans === "number"
    && typeof v.days_since_last_ban === "number"
    && typeof v.number_of_game_bans === "number"
    && typeof v.economy_ban === "string";
}

function readBanInfoCache(): Record<string, BanInfo> {
  try {
    const raw = localStorage.getItem(BAN_INFO_CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    if (!parsed || typeof parsed !== "object") return {};

    const entries = Object.entries(parsed).filter(([, value]) => isBanInfo(value));
    return Object.fromEntries(entries.map(([key, value]) => [key, value as BanInfo]));
  } catch {
    return {};
  }
}

function writeBanInfoCache(bans: Record<string, BanInfo>) {
  localStorage.setItem(BAN_INFO_CACHE_KEY, JSON.stringify(bans));
}

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

function toSteamAccountWarningPresentation(
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

function toWarningMap(
  banStates: Record<string, BanInfo>,
  t: PlatformUiCallbacks["t"],
): Record<string, AccountWarningPresentation> {
  const warnings: Record<string, AccountWarningPresentation> = {};
  for (const [accountId, banInfo] of Object.entries(banStates)) {
    const warning = toSteamAccountWarningPresentation(banInfo, t);
    if (warning) warnings[accountId] = warning;
  }
  return warnings;
}

export function getCachedSteamWarningStates(
  callbacks: PlatformUiCallbacks,
): Record<string, AccountWarningPresentation> {
  return toWarningMap(readBanInfoCache(), callbacks.t);
}

export async function loadSteamWarningStates(
  accounts: PlatformAccount[],
  options: PlatformWarningLoadOptions,
): Promise<Record<string, AccountWarningPresentation>> {
  const { forceRefresh = false, silent = true, t } = options;
  if (accounts.length === 0) return getCachedSteamWarningStates({ t });

  const steamIds = Array.from(new Set(accounts.map((account) => account.id)));
  const cachedBans = readBanInfoCache();
  const hasApiKeyConfigured = await hasApiKey().catch((e) => {
    console.error("[ban-check] failed to detect API key availability:", e);
    return false;
  });
  if (!hasApiKeyConfigured) {
    console.info("[ban-check] skipped: missing Steam API key");
    return toWarningMap(cachedBans, t);
  }

  const delayDays = getSettings().banCheckDays;
  const now = Date.now();
  const cachedState = readBanCheckState();
  const delayMs = delayDays * 24 * 60 * 60 * 1000;
  const withinDelayWindow = delayDays > 0
    && !!cachedState
    && now - cachedState.lastSuccessAt < delayMs;
  const cachedCheckedIds = new Set(cachedState?.checkedSteamIds ?? []);

  let idsToFetch: string[] = [];
  if (forceRefresh) {
    idsToFetch = steamIds;
  } else if (delayDays === 0) {
    idsToFetch = steamIds.filter((id) => !sessionBanCheckedIds.has(id));
  } else if (withinDelayWindow) {
    idsToFetch = steamIds.filter((id) => !cachedCheckedIds.has(id));
  } else {
    idsToFetch = steamIds;
  }

  if (idsToFetch.length === 0) {
    console.info("[ban-check] skipped: no accounts to check", {
      totalAccounts: steamIds.length,
      delayDays,
      forceRefresh,
    });
    return toWarningMap(cachedBans, t);
  }

  let checkingToastId: string | null = null;
  if (!silent) {
    if (activeBanCheckToastId) {
      removeToast(activeBanCheckToastId);
    }
    activeBanCheckToastId = addToast(t("toast.banChecking"), { durationMs: null });
    checkingToastId = activeBanCheckToastId;
  }

  try {
    const bans = await getPlayerBans(idsToFetch);
    let bannedCount = 0;
    const returnedIds = new Set<string>();
    let malformedRows = 0;

    for (const ban of bans) {
      if (typeof ban.steam_id !== "string" || ban.steam_id.length === 0) {
        malformedRows++;
        continue;
      }
      cachedBans[ban.steam_id] = ban;
      returnedIds.add(ban.steam_id);
      if (ban.vac_banned || ban.community_banned || ban.number_of_game_bans > 0) {
        bannedCount++;
      }
    }

    if (malformedRows > 0) {
      console.error("[ban-check] malformed ban rows without steam_id", {
        malformedRows,
        totalRows: bans.length,
        sampleRow: bans[0] ?? null,
      });
    }

    for (const steamId of idsToFetch) {
      sessionBanCheckedIds.add(steamId);
    }

    if (returnedIds.size !== idsToFetch.length) {
      const missingIds = idsToFetch.filter((steamId) => !returnedIds.has(steamId));
      console.warn("[ban-check] missing results for some Steam IDs", {
        expected: idsToFetch.length,
        received: returnedIds.size,
        missingIds,
      });
    }

    if (delayDays > 0) {
      const mergedCheckedIds = forceRefresh || !withinDelayWindow
        ? steamIds
        : Array.from(new Set([...(cachedState?.checkedSteamIds ?? []), ...idsToFetch]));
      writeBanCheckState({
        lastSuccessAt: now,
        checkedSteamIds: mergedCheckedIds,
      });
    } else {
      localStorage.removeItem(BAN_CHECK_STATE_KEY);
    }

    writeBanInfoCache(cachedBans);

    if (!silent && bannedCount > 0) {
      addToast(
        t(bannedCount > 1 ? "toast.banCheckSummary.multiple" : "toast.banCheckSummary.single", {
          count: bannedCount,
        }),
      );
    }
  } catch (e) {
    if (!silent && now - lastBanErrorToastAt >= BAN_ERROR_TOAST_COOLDOWN_MS) {
      addToast(t("toast.banCheckFailed", { error: String(e) }));
      lastBanErrorToastAt = now;
    }
    console.error("[ban-check] failed to fetch ban states:", e);
  } finally {
    if (checkingToastId && activeBanCheckToastId === checkingToastId) {
      removeToast(checkingToastId);
      activeBanCheckToastId = null;
    }
  }

  return toWarningMap(cachedBans, t);
}
