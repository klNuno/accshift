import { createGenericAdapter } from "$lib/platforms/genericAdapter";

export const discordAdapter = createGenericAdapter({
  id: "discord",
  reloadAfterAdd: true,
  noAccountsToastKey: "toast.noDiscordAccountsFound",
  noAccountsHintKey: "discord.noAccountsHint",
});
