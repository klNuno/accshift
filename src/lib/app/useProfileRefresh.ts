import type { AddToastOptions } from "$lib/features/notifications/store.svelte";
import type { MessageKey, TranslationParams } from "$lib/i18n";
import type {
  PlatformAdapter,
  PlatformDef,
  PlatformProfileInfo,
  WarningRefreshOutcome,
} from "$lib/shared/platform";

export type AvatarRefreshOutcome =
  | { kind: "complete"; count: number }
  | { kind: "failed" }
  | { kind: "partial"; ok: number; count: number };

/** A rejection or a resolved null is a miss: an adapter resolves null only
 *  for a failed fetch, and a real profile with no picture as
 *  `{ avatarUrl: null }`. */
export function avatarRefreshOutcome(
  settled: readonly PromiseSettledResult<PlatformProfileInfo | null>[],
): AvatarRefreshOutcome {
  const count = settled.length;
  let ok = 0;
  for (const result of settled) {
    if (result.status === "fulfilled" && result.value != null) ok += 1;
  }
  if (ok === count) return { kind: "complete", count };
  if (ok === 0) return { kind: "failed" };
  return { kind: "partial", ok, count };
}

type ProfileRefreshDeps = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  showToast: (message: string, options?: AddToastOptions) => unknown;
  /** Every platform definition; only those declaring the matching
   *  profileRefresh capability are refreshed. */
  platforms: readonly PlatformDef[];
  ensureAdapterReady: (platformId: string) => Promise<PlatformAdapter | null | undefined>;
  getActiveTab: () => string;
  loadAccounts: (
    silent?: boolean,
    showRefreshedToast?: boolean,
    forceRefresh?: boolean,
    checkBans?: boolean,
    deferBackground?: boolean,
  ) => Promise<unknown>;
};

/** The settings "refresh now" actions for avatars and bans (currently Steam only). */
export function createProfileRefresh({
  t,
  showToast,
  platforms,
  ensureAdapterReady,
  getActiveTab,
  loadAccounts,
}: ProfileRefreshDeps) {
  async function refreshAvatarsNow() {
    for (const def of platforms) {
      if (!def.capabilities?.profileRefresh?.avatars) continue;
      const adapter = await ensureAdapterReady(def.id);
      if (!adapter?.getProfileInfo) continue;
      try {
        const accounts = await adapter.loadAccounts();
        if (accounts.length === 0) {
          const noAccountsMsg = adapter.getNoAccountsToastMessage?.({ t });
          if (noAccountsMsg) showToast(noAccountsMsg);
          continue;
        }
        const settled = await Promise.allSettled(
          accounts.map((a) => adapter.getProfileInfo!(a.id)),
        );
        const outcome = avatarRefreshOutcome(settled);
        if (outcome.kind === "failed") {
          console.error("[avatars] refresh failed:", settled);
          showToast(t("toast.refreshFailed"), { type: "error" });
          continue;
        }
        if (getActiveTab() === def.id) void loadAccounts(true, false, true, false, false);
        if (outcome.kind === "partial") {
          console.error("[avatars] refresh incomplete:", settled);
          showToast(t("toast.refreshPartial", { ok: outcome.ok, count: outcome.count }), {
            type: "info",
          });
          continue;
        }
        showToast(t("toast.avatarRefreshComplete", { count: outcome.count }), {
          type: "success",
        });
      } catch (error) {
        console.error("[avatars] refresh failed:", error);
        showToast(t("toast.refreshFailed"), { type: "error" });
      }
    }
  }

  async function refreshBansNow() {
    for (const def of platforms) {
      if (!def.capabilities?.profileRefresh?.bans) continue;
      const adapter = await ensureAdapterReady(def.id);
      if (!adapter?.loadWarningStates) continue;
      try {
        const accounts = await adapter.loadAccounts();
        if (accounts.length === 0) {
          const noAccountsMsg = adapter.getNoAccountsToastMessage?.({ t });
          if (noAccountsMsg) showToast(noAccountsMsg);
          continue;
        }
        // Widened with `as`: the callback assigns it, which narrowing cannot see.
        let outcome = { kind: "complete", checked: accounts.length } as WarningRefreshOutcome;
        await adapter.loadWarningStates(accounts, {
          forceRefresh: true,
          silent: false,
          t,
          onSettled: (settled) => {
            outcome = settled;
          },
        });
        // The adapter already showed its own error toast. A second one, or a
        // success toast over the cached rows, would hide the failure.
        if (outcome.kind === "failed") continue;
        if (outcome.kind === "noApiKey") {
          showToast(t("toast.banRefreshNoApiKey"), { type: "error" });
          continue;
        }
        if (getActiveTab() === def.id) void loadAccounts(true, false, false, true, false);
        if (outcome.kind === "partial") {
          showToast(t("toast.refreshPartial", { ok: outcome.checked, count: outcome.requested }), {
            type: "info",
          });
          continue;
        }
        showToast(t("toast.banRefreshComplete", { count: accounts.length }), { type: "success" });
      } catch (error) {
        console.error("[bans] refresh failed:", error);
        showToast(t("toast.refreshFailed"), { type: "error" });
      }
    }
  }

  return { refreshAvatarsNow, refreshBansNow };
}
