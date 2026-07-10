import { createGenericAdapter } from "$lib/platforms/genericAdapter";

export const gogAdapter = createGenericAdapter({
  id: "gog",
  reloadAfterAdd: true,
  noAccountsToastKey: "toast.noGogAccountsFound",
  noAccountsHintKey: "gog.noAccountsHint",
});
