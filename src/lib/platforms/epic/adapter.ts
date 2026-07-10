import { createGenericAdapter } from "$lib/platforms/genericAdapter";

export const epicAdapter = createGenericAdapter({
  id: "epic",
  reloadAfterAdd: true,
  noAccountsToastKey: "toast.noEpicAccountsFound",
  noAccountsHintKey: "epic.noAccountsHint",
});
