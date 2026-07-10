import { createGenericAdapter } from "$lib/platforms/genericAdapter";

export const jagexAdapter = createGenericAdapter({
  id: "jagex",
  reloadAfterAdd: true,
  noAccountsToastKey: "toast.noJagexAccountsFound",
  noAccountsHintKey: "jagex.noAccountsHint",
});
