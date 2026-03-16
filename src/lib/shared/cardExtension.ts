import type { AccountWarningChip, AccountWarningTone } from "./accountWarnings";

export type CardExtensionChipTone = AccountWarningTone | "blue" | "green" | "slate";

export interface CardExtensionChip {
  text: string;
  tone: CardExtensionChipTone;
  onClick?: () => void;
}

export interface CardExtensionLink {
  label: string;
  url: string;
}

export interface CardExtensionSection {
  title?: string;
  text?: string;
  lines?: string[];
  link?: CardExtensionLink;
  chips?: CardExtensionChip[];
  loading?: boolean;
}

export interface CardExtensionContent {
  sections: CardExtensionSection[];
}

export function hasCardExtensionContent(content: CardExtensionContent | null | undefined): boolean {
  if (!content) return false;
  return content.sections.some((section) =>
    Boolean(
      (section.title && section.title.trim())
      || (section.text && section.text.trim())
      || (section.lines && section.lines.some((line) => line.trim()))
      || (section.chips && section.chips.length > 0)
      || section.loading
    )
  );
}

export function warningChipsToExtensionChips(chips: AccountWarningChip[] | undefined): CardExtensionChip[] {
  return (chips ?? []).map((chip) => ({ text: chip.text, tone: chip.tone }));
}
