export interface ContextMenuItem {
  label?: string;
  action?: () => void;
  separator?: true;
  submenu?: ContextMenuItem[];
  submenuLoader?: () => Promise<ContextMenuItem[]>;
  swatches?: Array<{
    id: string;
    label: string;
    color: string;
    active?: boolean;
    action: () => void;
  }>;
}

export interface InputDialogConfig {
  title: string;
  placeholder: string;
  initialValue: string;
  allowEmpty?: boolean;
  onConfirm: (value: string) => void;
}
