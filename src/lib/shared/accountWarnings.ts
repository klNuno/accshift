export type AccountWarningTone = "red" | "orange";

export interface AccountWarningChip {
  tone: AccountWarningTone;
  text: string;
}

export interface AccountWarningPresentation {
  tooltipText: string;
  cardOutlineTone?: AccountWarningTone | null;
  listHasRed?: boolean;
  listHasOrange?: boolean;
  chips?: AccountWarningChip[];
}
