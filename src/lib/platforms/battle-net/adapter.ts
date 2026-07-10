import type { PlatformAccount } from "$lib/shared/platform";
import { createGenericAdapter } from "$lib/platforms/genericAdapter";

interface BattleNetAccount {
  email: string;
  battleTag?: string;
  lastLoginAt?: number | null;
}

function getBattleNetDisplayName(email: string): string {
  const trimmed = email.trim();
  const candidate = trimmed.split("@")[0]?.trim();
  return candidate || trimmed;
}

function getBattleNetLabel(account: BattleNetAccount): string {
  const battleTag = (account.battleTag ?? "").trim();
  if (battleTag) {
    return battleTag.split("#")[0]?.trim() || battleTag;
  }
  return getBattleNetDisplayName(account.email);
}

function toAccount(account: BattleNetAccount): PlatformAccount {
  return {
    id: account.email,
    displayName: getBattleNetLabel(account),
    username: "",
    lastLoginAt: account.lastLoginAt ?? null,
  };
}

// Keep raw emails out of log files: only the first chars of the local part are logged.
function maskEmail(email: string): string {
  const local = email.split("@")[0] ?? "";
  return `${local.slice(0, 3)}…`;
}

export const battleNetAdapter = createGenericAdapter<BattleNetAccount>({
  id: "battle-net",
  i18nPrefix: "battlenet",
  noAccountsToastKey: "toast.noBattleNetAccountsFound",
  toAccount,
  supportsAccountLabels: false,
  maskSwitchLogId: maskEmail,
  copyItems: (account) => {
    const username = (account.displayName || account.username || account.id).trim() || account.id;
    return [
      {
        field: "username",
        value: username,
        labelKey: "battlenet.copyLabelUsername",
        clipboardLabelKey: "battlenet.copyLabelUsername",
      },
      {
        field: "email",
        value: account.id,
        labelKey: "battlenet.copyLabelEmail",
        clipboardLabelKey: "battlenet.copyLabelEmail",
      },
    ];
  },
  forgetToastParams: (display) => ({ email: display }),
});
