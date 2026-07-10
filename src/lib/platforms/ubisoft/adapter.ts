import type { PlatformAccount } from "$lib/shared/platform";
import { createGenericAdapter } from "$lib/platforms/genericAdapter";

interface UbisoftAccount {
  uuid: string;
  label: string;
  lastUsedAt?: number | null;
  snapshotSaved: boolean;
}

function getDisplayName(account: UbisoftAccount): string {
  const label = (account.label ?? "").trim();
  if (label) return label;
  // Shorten UUID for display: first 8 chars
  return account.uuid.split("-")[0] ?? account.uuid;
}

function toAccount(account: UbisoftAccount): PlatformAccount {
  return {
    id: account.uuid,
    displayName: getDisplayName(account),
    username: "",
    lastLoginAt: account.lastUsedAt ?? null,
  };
}

export const ubisoftAdapter = createGenericAdapter<UbisoftAccount>({
  id: "ubisoft",
  noAccountsToastKey: "toast.noUbisoftAccountsFound",
  toAccount,
  copyItems: (account) => [
    {
      field: "uuid",
      value: account.id,
      labelKey: "ubisoft.copyLabelUuid",
      clipboardLabelKey: "ubisoft.copyLabelUuid",
    },
  ],
});
