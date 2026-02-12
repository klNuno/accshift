export interface ContextMenuItem {
  label?: string;
  action?: () => void;
  separator?: true;
}

export interface InputDialogConfig {
  title: string;
  placeholder: string;
  initialValue: string;
  onConfirm: (value: string) => void;
}
