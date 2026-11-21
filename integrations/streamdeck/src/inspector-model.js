// What the property inspector shows, without the DOM: the option lists and
// the settings a choice writes. src/inspector.js draws them.

import { translate } from "./i18n.js";

/**
 * Options of the platform list; a saved platform the CLI no longer lists
 * stays selectable so the key keeps its choice.
 * @returns {{ value: string, label: string, selected: boolean, disabled?: boolean }[]}
 */
export function platformOptions(language, items, selected) {
  const options = [
    {
      value: "",
      label: translate(language, "pi.choosePlatform"),
      selected: !selected,
      disabled: true,
    },
  ];
  for (const item of items) {
    options.push({ value: item.id, label: item.name, selected: item.id === selected });
  }
  if (selected && !items.some((item) => item.id === selected)) {
    options.push({ value: selected, label: selected, selected: true });
  }
  return options;
}

/** Options of the account list, the signed-in account marked. */
export function accountOptions(language, items, settings) {
  const selected = settings.accountId ?? "";
  const options = [
    {
      value: "",
      label: translate(language, items.length ? "pi.chooseAccount" : "pi.noAccounts"),
      selected: !selected,
      disabled: true,
    },
  ];
  for (const item of items) {
    const label = item.current ? translate(language, "pi.signedIn", { name: item.name }) : item.name;
    options.push({ value: item.id, label, selected: item.id === selected });
  }
  if (selected && !items.some((item) => item.id === selected)) {
    options.push({ value: selected, label: settings.accountName || selected, selected: true });
  }
  return options;
}

/** The settings a platform choice writes: the account is cleared. */
export function settingsForPlatform(platform) {
  return { platform, accountId: "", accountName: "", avatarPath: "" };
}

/** The settings an account choice writes. */
export function settingsForAccount(platform, account) {
  return {
    platform,
    accountId: account.id,
    accountName: account.name,
    avatarPath: account.avatarPath ?? "",
  };
}
