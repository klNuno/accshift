import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { getPlatformDefinition } from "$lib/platforms/registry";
import { isPlatformUsable } from "$lib/app/platformShell.svelte";
import { parseDeepLink, type DeepLinkSwitchRequest } from "$lib/app/deepLinkUrl";
import type { PlatformAccount } from "$lib/shared/platform";
import type { AppSettings } from "$lib/features/settings/types";
import type { RuntimeOs } from "$lib/shared/platform";
import type { MessageKey, TranslationParams } from "$lib/i18n";

const BOOT_TIMEOUT_MS = 20000;
const LOAD_TIMEOUT_MS = 15000;
const POLL_INTERVAL_MS = 100;

// Looks up a deep-link account ref. The id form is authoritative (ids are
// unique), so an id match always wins. Username/displayName are user-editable
// text and can collide across accounts (renames, duplicate imports): if more
// than one account matches on either of those, the ref is ambiguous and we
// treat it the same as "not found" rather than silently guessing which
// account the link meant.
function findAccount(accounts: PlatformAccount[], ref: string): PlatformAccount | undefined {
  const needle = ref.trim().toLowerCase();

  const byId = accounts.find((account) => account.id.trim().toLowerCase() === needle);
  if (byId) return byId;

  const byUsername = accounts.filter((account) => account.username.trim().toLowerCase() === needle);
  if (byUsername.length === 1) return byUsername[0];
  if (byUsername.length > 1) return undefined;

  const byDisplayName = accounts.filter(
    (account) => (account.displayName || "").trim().toLowerCase() === needle,
  );
  if (byDisplayName.length === 1) return byDisplayName[0];
  return undefined;
}

async function waitUntil(condition: () => boolean, timeoutMs: number): Promise<boolean> {
  const start = Date.now();
  while (!condition()) {
    if (Date.now() - start > timeoutMs) return false;
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
  }
  return true;
}

type DeepLinkDeps = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  showToast: (message: string) => void;
  getSettings: () => AppSettings;
  getRuntimeOs: () => RuntimeOs;
  getActiveTab: () => string;
  isPinLocked: () => boolean;
  isBootReady: () => boolean;
  changeTab: (tab: string) => Promise<void>;
  loadAccounts: () => Promise<unknown> | void;
  getAccounts: () => PlatformAccount[];
  isLoaderLoading: () => boolean;
  getLoaderError: () => string | null;
  switchToAccount: (account: PlatformAccount) => Promise<void>;
  // Optional gate asked right before a deep-link-triggered switch runs, so a
  // link can be required to go through an explicit user confirmation instead
  // of switching accounts unattended. Return false to cancel the switch;
  // dropped silently, the same way the PIN-lock gate above is. When the host
  // does not wire this in, the switch proceeds as before (no gate).
  confirmSwitch?: (account: PlatformAccount, platformName: string) => Promise<boolean> | boolean;
};

export function createDeepLinkController(deps: DeepLinkDeps) {
  let unlisten: (() => void) | null = null;
  let started = false;
  let busy = false;
  // A link that arrives while another one is still being handled is queued
  // instead of dropped, keeping only the most recent one (an automation
  // firing twice in a row wants "switch to the latest ref", not a replay of
  // every intermediate request).
  let pendingUrl: string | null = null;

  async function handleSwitch({ platformId, accountRef }: DeepLinkSwitchRequest) {
    // A deep link can be the app's launch trigger; the shell isn't usable yet.
    if (!(await waitUntil(deps.isBootReady, BOOT_TIMEOUT_MS))) return;
    // No switching from behind the PIN lock: an accshift:// link must not
    // bypass what the lock is for. Dropped silently, the screen is obscured.
    if (deps.isPinLocked()) return;

    const settings = deps.getSettings();
    if (!settings.deepLinksEnabled) {
      deps.showToast(deps.t("toast.deepLinkDisabled"));
      return;
    }
    const platformDef = getPlatformDefinition(platformId);
    if (!platformDef) {
      deps.showToast(deps.t("toast.deepLinkUnknownPlatform", { platform: platformId }));
      return;
    }
    if (!isPlatformUsable(platformId, deps.getRuntimeOs())) {
      deps.showToast(deps.t("app.platformUnsupportedOs", { platform: platformDef.name }));
      return;
    }
    if (!settings.enabledPlatforms.includes(platformId)) {
      deps.showToast(deps.t("toast.deepLinkPlatformDisabled", { platform: platformDef.name }));
      return;
    }

    if (deps.getActiveTab() !== platformId) {
      await deps.changeTab(platformId);
    }
    await waitUntil(() => !deps.isLoaderLoading(), LOAD_TIMEOUT_MS);

    let account = findAccount(deps.getAccounts(), accountRef);
    if (!account) {
      // The account may have been added since the last load (e.g. via CLI).
      await deps.loadAccounts();
      account = findAccount(deps.getAccounts(), accountRef);
    }
    if (!account) {
      deps.showToast(
        deps.t("toast.deepLinkAccountNotFound", {
          platform: platformDef.name,
          account: accountRef,
        }),
      );
      return;
    }

    if (deps.confirmSwitch) {
      const allowed = await deps.confirmSwitch(account, platformDef.name);
      if (!allowed) return;
    }

    await deps.switchToAccount(account);
    if (!deps.getLoaderError()) {
      deps.showToast(
        deps.t("toast.deepLinkSwitched", {
          account: account.displayName || account.username || account.id,
        }),
      );
    }
  }

  async function handleUrl(rawUrl: string) {
    if (busy) {
      pendingUrl = rawUrl;
      return;
    }
    busy = true;
    try {
      await waitUntil(deps.isBootReady, BOOT_TIMEOUT_MS);
      const request = parseDeepLink(rawUrl);
      if (!request) {
        if (deps.getSettings().deepLinksEnabled) {
          deps.showToast(deps.t("toast.deepLinkInvalid"));
        }
        return;
      }
      await handleSwitch(request);
    } catch (error) {
      console.error("Deep link handling failed:", error);
    } finally {
      busy = false;
      if (pendingUrl) {
        const next = pendingUrl;
        pendingUrl = null;
        void handleUrl(next);
      }
    }
  }

  async function start() {
    if (started) return;
    started = true;
    try {
      // onOpenUrl replays the launch URL (cold start) then listens for
      // runtime ones (second instance argv, macOS open-url events).
      unlisten = await onOpenUrl((urls) => {
        for (const url of urls) {
          void handleUrl(url);
        }
      });
    } catch (error) {
      console.error("Failed to start deep link listener:", error);
    }
  }

  function stop() {
    unlisten?.();
    unlisten = null;
    started = false;
  }

  return { start, stop };
}
