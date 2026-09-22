import type { PlatformAccount } from "$lib/shared/platform";
import { createGenericAdapter } from "$lib/platforms/genericAdapter";
import { unixMsToSeconds } from "$lib/shared/time";

/** Backend payload of `platform_get_accounts` for Ubisoft Connect. */
export interface UbisoftRawAccount {
  uuid: string;
  label: string;
  /** Unix MILLISECONDS (`platforms::now_unix_ms`). */
  lastUsedAt?: number | null;
  snapshotSaved: boolean;
}

function getDisplayName(account: UbisoftRawAccount): string {
  const label = (account.label ?? "").trim();
  if (label) return label;
  // Shorten UUID for display: first 8 chars
  return account.uuid.split("-")[0] ?? account.uuid;
}

export function toUbisoftAccount(account: UbisoftRawAccount): PlatformAccount {
  return {
    id: account.uuid,
    displayName: getDisplayName(account),
    username: "",
    lastLoginAtSec: unixMsToSeconds(account.lastUsedAt),
  };
}

export const ubisoftAdapter = createGenericAdapter<UbisoftRawAccount>({
  id: "ubisoft",
  noAccountsToastKey: "toast.noUbisoftAccountsFound",
  toAccount: toUbisoftAccount,
  copyItems: (account) => [
    {
      field: "uuid",
      value: account.id,
      labelKey: "ubisoft.copyLabelUuid",
      clipboardLabelKey: "ubisoft.copyLabelUuid",
    },
  ],
});
