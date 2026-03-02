import type { MessageKey, TranslationParams } from "$lib/i18n";
import { ACCOUNT_CARD_COLOR_PRESETS } from "../accountCardColors";
import type { PlatformAccount } from "../platform";
import type { ContextMenuAction } from "./types";

const DEFAULT_COLOR_LABEL_KEYS = {
  none: "color.none",
  slate: "color.slate",
  blue: "color.blue",
  cyan: "color.cyan",
  emerald: "color.emerald",
  amber: "color.amber",
  rose: "color.rose",
  violet: "color.violet",
  zinc: "color.zinc",
} as const satisfies Record<(typeof ACCOUNT_CARD_COLOR_PRESETS)[number]["id"], MessageKey>;

export interface AccountAppearanceActionCallbacks {
  t: (key: MessageKey, params?: TranslationParams) => string;
  getCurrentColor: () => string;
  getExistingNote: () => string;
  openNoteEditor: (initialNote: string) => void;
  setColor: (color: string) => void;
  getColorLabel?: (presetId: (typeof ACCOUNT_CARD_COLOR_PRESETS)[number]["id"]) => string;
}

export function getAccountAppearanceContextActions(
  account: PlatformAccount,
  callbacks: AccountAppearanceActionCallbacks,
): ContextMenuAction[] {
  const existingNote = callbacks.getExistingNote();
  const currentColor = callbacks.getCurrentColor();
  const getColorLabel = callbacks.getColorLabel
    ?? ((presetId) => callbacks.t(DEFAULT_COLOR_LABEL_KEYS[presetId]));

  return [
    {
      id: `account.appearance.${account.id}`,
      group: "account.appearance",
      label: callbacks.t("context.menu.editCardAndColor"),
      submenu: [
        {
          id: `account.appearance.note.${account.id}`,
          label: existingNote ? callbacks.t("context.menu.editNote") : callbacks.t("context.menu.addNote"),
          action: () => callbacks.openNoteEditor(existingNote),
        },
        {
          id: `account.appearance.color.${account.id}`,
          kind: "swatches",
          label: callbacks.t("context.menu.cardColor"),
          swatches: ACCOUNT_CARD_COLOR_PRESETS.map((preset) => ({
            id: preset.id,
            label: getColorLabel(preset.id),
            color: preset.color,
            active: currentColor === preset.color,
            action: () => callbacks.setColor(preset.color),
          })),
        },
      ],
    },
  ];
}
