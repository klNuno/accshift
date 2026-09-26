import { PASS } from "$lib/shared/keyboard/controller";
import type { KeyScope, ShortcutBinding } from "$lib/shared/keyboard/types";
import type { ItemRef } from "$lib/features/folders/types";
import { pickCycledTab } from "./tabCycle";

/** Scopes where the account grid takes arrow keys and Enter (virtual roving focus). */
export const CARD_NAV_SCOPES: KeyScope[] = ["app", "bulk-edit"];

type KeyScopeDeps = {
  /** PIN lock, streamer overlay or suspended render: nothing below is reachable. */
  isLocked: () => boolean;
  isPaletteOpen: () => boolean;
  isOnboardingOpen: () => boolean;
  hasDialog: () => boolean;
  hasContextMenu: () => boolean;
  isBulkEditMode: () => boolean;
  isSettingsOpen: () => boolean;
  isPersonasOpen: () => boolean;
};

/** The topmost layer owns the keyboard. Checked in order, first hit wins. */
export function createKeyScopeResolver(deps: KeyScopeDeps): () => KeyScope {
  return function currentKeyScope(): KeyScope {
    if (deps.isLocked()) return "locked";
    if (deps.isPaletteOpen()) return "palette";
    if (deps.isOnboardingOpen()) return "onboarding";
    if (deps.hasDialog()) return "dialog";
    if (deps.hasContextMenu()) return "context-menu";
    if (deps.isBulkEditMode()) return "bulk-edit";
    if (deps.isSettingsOpen()) return "settings";
    if (deps.isPersonasOpen()) return "personas";
    return "app";
  };
}

type CardFocusPort = {
  readonly focusedId: string | null;
  readonly focusedItem: ItemRef | null;
  clear: () => void;
  move: (direction: "left" | "right" | "up" | "down") => boolean;
  ownsActivationKey: (target: EventTarget | null) => boolean;
};

export type KeyboardBindingDeps = {
  cardFocus: CardFocusPort;
  closeContextMenu: () => void;
  setPaletteOpen: (open: boolean) => void;
  /** Cancels a card drag in flight; true when there was one. */
  cancelDragFromEscape: () => boolean;
  hasInputDialog: () => boolean;
  closeInputDialog: () => void;
  closeConfirmDialog: () => void;
  /** The persona wizard's own Escape handling; truthy when it consumed the key. */
  personasPanelEscape: () => boolean | undefined;
  closePersonas: () => void;
  getActiveElement: () => Element | null;
  getSearchInput: () => HTMLInputElement | null;
  clearSearchQuery: () => void;
  focusSettingsSearch: () => void;
  addAccount: () => void;
  newFolder: () => void;
  refresh: () => void;
  isBulkEditToggleAllowed: () => boolean;
  toggleBulkEdit: () => void;
  toggleSettingsPanel: () => unknown;
  openPersonas: () => void;
  toggleViewMode: () => void;
  getEnabledPlatforms: () => readonly { id: string }[];
  getUnavailablePlatformIds: () => ReadonlySet<string>;
  getActiveTab: () => string;
  changeTab: (tab: string) => unknown;
  zoomIn: () => void;
  zoomOut: () => void;
  resetZoom: () => void;
  getCurrentFolderId: () => string | null;
  navigateBack: () => void;
  activateFocusedCard: () => boolean;
  renameFocusedCard: () => boolean;
  openFocusedCardContextMenu: () => boolean;
  toggleBulkEditAccount: (accountId: string) => void;
  bulkEditSelectAll: () => void;
  bulkEditDeselectAll: () => void;
};

/** The app's whole shortcut table, in match order: the first binding of a
 *  combo whose scope matches and that does not return PASS takes the key. */
export function createKeyboardBindings(deps: KeyboardBindingDeps): ShortcutBinding[] {
  const { cardFocus } = deps;

  function focusSearch() {
    deps.getSearchInput()?.focus();
    deps.getSearchInput()?.select();
  }

  function cycleTab(direction: 1 | -1) {
    const usable = deps
      .getEnabledPlatforms()
      .filter((p) => !deps.getUnavailablePlatformIds().has(p.id))
      .map((p) => p.id);
    const next = pickCycledTab(usable, deps.getActiveTab(), direction);
    if (!next) return;
    deps.closePersonas();
    void deps.changeTab(next);
  }

  return [
    // WebView built-ins that must never fire in a desktop app shell:
    // Ctrl+W closes the window, Ctrl+P prints, F3/Ctrl+G open the native
    // find bar, Ctrl+U/Ctrl+J open browser panels, F7 toggles caret mode.
    { combo: "mod+w", scopes: ["*"], run: () => {} },
    { combo: "mod+p", scopes: ["*"], run: () => {} },
    { combo: "f3", scopes: ["*"], allowInInput: true, run: () => {} },
    { combo: "mod+g", scopes: ["*"], run: () => {} },
    { combo: "f7", scopes: ["*"], allowInInput: true, run: () => {} },
    { combo: "mod+u", scopes: ["*"], run: () => {} },
    { combo: "mod+j", scopes: ["*"], run: () => {} },
    // Alt+Right would trigger WebView forward-history through our pushState
    // entries; Alt+Left is repurposed below and swallowed everywhere else.
    { combo: "alt+arrowright", scopes: ["*"], allowInInput: true, run: () => {} },
    { combo: "alt+arrowleft", scopes: ["*"], allowInInput: true, run: () => PASS },

    // Command palette.
    {
      combo: "mod+k",
      scopes: ["app", "settings", "personas", "bulk-edit", "context-menu"],
      run: () => {
        deps.closeContextMenu();
        deps.setPaletteOpen(true);
      },
    },
    {
      combo: "mod+k",
      scopes: ["palette"],
      allowInInput: true,
      run: () => {
        deps.setPaletteOpen(false);
      },
    },

    // Escape cascade: exactly one layer closes per press. Scopes whose owner
    // component already handles Escape correctly (bulk edit steps, settings)
    // return PASS so the legacy handler still runs, but only for them.
    // A card drag in flight is cancelled first, whatever the scope.
    {
      combo: "escape",
      scopes: ["*"],
      allowInInput: true,
      run: () => (deps.cancelDragFromEscape() ? undefined : PASS),
    },
    {
      combo: "escape",
      scopes: ["palette"],
      allowInInput: true,
      run: () => {
        deps.setPaletteOpen(false);
      },
    },
    {
      combo: "escape",
      scopes: ["dialog"],
      allowInInput: true,
      run: () => {
        if (deps.hasInputDialog()) deps.closeInputDialog();
        else deps.closeConfirmDialog();
      },
    },
    { combo: "escape", scopes: ["context-menu"], allowInInput: true, run: () => PASS },
    { combo: "escape", scopes: ["bulk-edit"], allowInInput: true, run: () => PASS },
    { combo: "escape", scopes: ["settings"], allowInInput: true, run: () => PASS },
    { combo: "escape", scopes: ["onboarding", "locked"], allowInInput: true, run: () => PASS },
    {
      combo: "escape",
      scopes: ["personas"],
      allowInInput: true,
      run: () => {
        // The persona wizard steps back (or asks) instead of losing its input.
        if (deps.personasPanelEscape()) return;
        deps.closePersonas();
      },
    },
    {
      combo: "escape",
      scopes: ["app"],
      allowInInput: true,
      run: () => {
        const active = deps.getActiveElement();
        if (active instanceof HTMLInputElement && active === deps.getSearchInput()) {
          deps.clearSearchQuery();
          active.blur();
          return;
        }
        if (cardFocus.focusedId) {
          cardFocus.clear();
          return;
        }
        return PASS;
      },
    },

    // App-level shortcuts.
    { combo: "mod+f", scopes: ["app", "bulk-edit"], run: focusSearch },
    { combo: "mod+f", scopes: ["settings"], run: () => deps.focusSettingsSearch() },
    // Swallow mod+f everywhere else so the WebView2 native find bar never opens.
    { combo: "mod+f", scopes: ["*"], run: () => {} },
    { combo: "mod+n", scopes: ["app"], run: deps.addAccount },
    { combo: "mod+shift+n", scopes: ["app"], run: () => deps.newFolder() },
    { combo: "mod+r", scopes: ["app"], run: deps.refresh },
    { combo: "f5", scopes: ["app"], allowInInput: true, run: deps.refresh },
    { combo: "f5", scopes: ["*"], allowInInput: true, run: () => {} },
    { combo: "mod+shift+r", scopes: ["*"], run: () => {} },
    {
      combo: "mod+e",
      scopes: ["app", "bulk-edit"],
      run: () => {
        if (deps.isBulkEditToggleAllowed()) deps.toggleBulkEdit();
      },
    },
    {
      combo: "mod+,",
      scopes: ["app", "settings", "personas"],
      run: () => {
        deps.closePersonas();
        void deps.toggleSettingsPanel();
      },
    },
    { combo: "mod+shift+p", scopes: ["app"], run: deps.openPersonas },
    {
      combo: "mod+shift+l",
      scopes: ["app"],
      run: () => deps.toggleViewMode(),
    },
    { combo: "mod+tab", scopes: ["app"], allowInInput: true, run: () => cycleTab(1) },
    { combo: "mod+shift+tab", scopes: ["app"], allowInInput: true, run: () => cycleTab(-1) },
    ...Array.from({ length: 9 }, (_, i): ShortcutBinding => ({
      combo: `mod+digit${i + 1}`,
      scopes: ["app", "settings", "personas"],
      run: () => {
        const platform = deps.getEnabledPlatforms()[i];
        if (!platform || deps.getUnavailablePlatformIds().has(platform.id)) return;
        deps.closePersonas();
        void deps.changeTab(platform.id);
      },
    })),
    { combo: "mod+plus", scopes: ["*"], run: deps.zoomIn },
    // Layouts where "+" is a shifted key (AZERTY and friends).
    { combo: "mod+shift+plus", scopes: ["*"], run: deps.zoomIn },
    { combo: "mod+minus", scopes: ["*"], run: deps.zoomOut },
    { combo: "mod+digit0", scopes: ["*"], run: deps.resetZoom },
    {
      combo: "alt+arrowleft",
      scopes: ["app"],
      allowInInput: true,
      run: () => {
        if (deps.getCurrentFolderId()) deps.navigateBack();
      },
    },
    {
      combo: "backspace",
      scopes: ["app"],
      run: () => {
        if (!deps.getCurrentFolderId()) return PASS;
        deps.navigateBack();
      },
    },

    // Card focus navigation (virtual roving focus, also live in bulk edit).
    {
      combo: "arrowleft",
      scopes: CARD_NAV_SCOPES,
      run: () => (cardFocus.move("left") ? undefined : PASS),
    },
    {
      combo: "arrowright",
      scopes: CARD_NAV_SCOPES,
      run: () => (cardFocus.move("right") ? undefined : PASS),
    },
    {
      combo: "arrowup",
      scopes: CARD_NAV_SCOPES,
      run: () => (cardFocus.move("up") ? undefined : PASS),
    },
    {
      combo: "arrowdown",
      scopes: CARD_NAV_SCOPES,
      run: () => (cardFocus.move("down") ? undefined : PASS),
    },
    // Enter and Space act on the virtual focus only when no other control
    // holds real focus: a focused button keeps its own activation.
    {
      combo: "enter",
      scopes: CARD_NAV_SCOPES,
      run: (e) =>
        cardFocus.ownsActivationKey(e.target) && deps.activateFocusedCard() ? undefined : PASS,
    },
    { combo: "f2", scopes: ["app"], run: () => (deps.renameFocusedCard() ? undefined : PASS) },
    {
      combo: "delete",
      scopes: ["app"],
      run: () => (deps.openFocusedCardContextMenu() ? undefined : PASS),
    },
    {
      combo: "shift+f10",
      scopes: ["app"],
      run: () => (deps.openFocusedCardContextMenu() ? undefined : PASS),
    },
    {
      combo: "contextmenu",
      scopes: ["app"],
      run: () => (deps.openFocusedCardContextMenu() ? undefined : PASS),
    },
    {
      combo: "space",
      scopes: ["bulk-edit"],
      run: (e) => {
        if (!cardFocus.ownsActivationKey(e.target)) return PASS;
        const item = cardFocus.focusedItem;
        if (!item || item.type !== "account") return PASS;
        deps.toggleBulkEditAccount(item.id);
      },
    },

    // Bulk edit selection. Skipped inside a text field (the bulk edit
    // settings form, the search box) so Ctrl+A selects text there.
    { combo: "mod+a", scopes: ["bulk-edit"], skipInInput: true, run: deps.bulkEditSelectAll },
    { combo: "mod+d", scopes: ["bulk-edit"], skipInInput: true, run: deps.bulkEditDeselectAll },
  ];
}
