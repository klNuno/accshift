import type { ContextMenuItem } from "../types";

export const ACCOUNT_CONTEXT_GROUP_ORDER = [
  "platform.primary",
  "platform.copy",
  "platform.data",
  "platform.danger",
  "account.appearance",
] as const;

export interface ContextMenuSwatchAction {
  id: string;
  label: string;
  color: string;
  active?: boolean;
  action: () => void | Promise<void>;
}

interface ContextMenuBaseAction {
  id: string;
  group?: string;
}

export interface ContextMenuItemAction extends ContextMenuBaseAction {
  kind?: "item";
  label: string;
  action?: () => void | Promise<void>;
  submenu?: ContextMenuAction[];
  submenuLoader?: () => Promise<ContextMenuAction[]>;
}

export interface ContextMenuSwatchesAction extends ContextMenuBaseAction {
  kind: "swatches";
  label: string;
  swatches: ContextMenuSwatchAction[];
}

export type ContextMenuAction = ContextMenuItemAction | ContextMenuSwatchesAction;

function toContextMenuItem(
  action: ContextMenuAction,
  groupOrder: readonly string[],
): ContextMenuItem {
  const submenuLoader = action.kind !== "swatches" ? action.submenuLoader : undefined;

  if (action.kind === "swatches") {
    return {
      label: action.label,
      swatches: action.swatches,
    };
  }

  return {
    label: action.label,
    action: action.action,
    submenu: action.submenu ? buildContextMenuItems(action.submenu, groupOrder) : undefined,
    submenuLoader: submenuLoader
      ? async () => buildContextMenuItems(await submenuLoader(), groupOrder)
      : undefined,
  };
}

export function buildContextMenuItems(
  actions: ContextMenuAction[],
  groupOrder: readonly string[] = [],
): ContextMenuItem[] {
  if (actions.length === 0) return [];

  const buckets = new Map<string, ContextMenuAction[]>();
  const firstSeenOrder = new Map<string, number>();

  actions.forEach((action, index) => {
    const key = action.group || "__default__";
    if (!buckets.has(key)) {
      buckets.set(key, []);
      firstSeenOrder.set(key, index);
    }
    buckets.get(key)?.push(action);
  });

  const orderedGroups = Array.from(buckets.keys()).sort((a, b) => {
    const aPriority = groupOrder.indexOf(a);
    const bPriority = groupOrder.indexOf(b);
    if (aPriority !== -1 || bPriority !== -1) {
      if (aPriority === -1) return 1;
      if (bPriority === -1) return -1;
      return aPriority - bPriority;
    }
    return (firstSeenOrder.get(a) ?? 0) - (firstSeenOrder.get(b) ?? 0);
  });

  const items: ContextMenuItem[] = [];

  orderedGroups.forEach((group, groupIndex) => {
    const groupActions = buckets.get(group) ?? [];
    if (groupActions.length === 0) return;
    if (groupIndex > 0) {
      items.push({ separator: true });
    }
    items.push(...groupActions.map((action) => toContextMenuItem(action, groupOrder)));
  });

  return items;
}
