import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createKeyboardController } from "$lib/shared/keyboard/controller";
import type { KeyScope } from "$lib/shared/keyboard/types";
import type { ItemRef } from "$lib/features/folders/types";
import {
  CARD_NAV_SCOPES,
  createKeyboardBindings,
  createKeyScopeResolver,
  type KeyboardBindingDeps,
} from "./keyboardBindings";

// Vitest runs in node: stand in for the DOM classes the controller and the
// Escape binding check.
class FakeHTMLElement {
  isContentEditable = false;
  constructor(public tagName: string) {}
}
class FakeHTMLInputElement extends FakeHTMLElement {
  blur = vi.fn();
  focus = vi.fn();
  select = vi.fn();
  constructor() {
    super("INPUT");
  }
}

type Listener = (e: KeyboardEvent) => void;
let listener: Listener | null = null;

beforeEach(() => {
  listener = null;
  vi.stubGlobal("HTMLElement", FakeHTMLElement);
  vi.stubGlobal("HTMLInputElement", FakeHTMLInputElement);
  vi.stubGlobal("window", {
    addEventListener: (_type: string, fn: Listener) => {
      listener = fn;
    },
    removeEventListener: () => {
      listener = null;
    },
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

type KeyOpts = {
  code?: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  target?: unknown;
};

function key(k: string, opts: KeyOpts = {}) {
  const event = {
    key: k,
    code: opts.code ?? "",
    ctrlKey: opts.ctrl ?? false,
    metaKey: false,
    shiftKey: opts.shift ?? false,
    altKey: opts.alt ?? false,
    target: opts.target ?? null,
    handled: false,
    preventDefault() {
      event.handled = true;
    },
    stopPropagation() {},
  };
  return event;
}

function setup(overrides: Partial<KeyboardBindingDeps> = {}) {
  const focus = {
    focusedId: null as string | null,
    focusedItem: null as ItemRef | null,
    ownsKey: true,
    moves: true,
  };
  const cardFocus = {
    get focusedId() {
      return focus.focusedId;
    },
    get focusedItem() {
      return focus.focusedItem;
    },
    clear: vi.fn(() => {
      focus.focusedId = null;
    }),
    move: vi.fn(() => focus.moves),
    ownsActivationKey: vi.fn(() => focus.ownsKey),
  };
  const state = {
    scope: "app" as KeyScope,
    folderId: null as string | null,
    activeTab: "steam",
    searchInput: null as FakeHTMLInputElement | null,
    activeElement: null as unknown,
    unavailable: new Set<string>(),
  };
  const deps: KeyboardBindingDeps = {
    cardFocus,
    closeContextMenu: vi.fn(),
    setPaletteOpen: vi.fn(),
    cancelDragFromEscape: vi.fn(() => false),
    hasInputDialog: vi.fn(() => false),
    closeInputDialog: vi.fn(),
    closeConfirmDialog: vi.fn(),
    personasPanelEscape: vi.fn(() => false),
    closePersonas: vi.fn(),
    getActiveElement: () => state.activeElement as Element | null,
    getSearchInput: () => state.searchInput as unknown as HTMLInputElement | null,
    clearSearchQuery: vi.fn(),
    focusSettingsSearch: vi.fn(),
    addAccount: vi.fn(),
    newFolder: vi.fn(),
    refresh: vi.fn(),
    isBulkEditToggleAllowed: vi.fn(() => true),
    toggleBulkEdit: vi.fn(),
    toggleSettingsPanel: vi.fn(),
    openPersonas: vi.fn(),
    toggleViewMode: vi.fn(),
    getEnabledPlatforms: () => [{ id: "steam" }, { id: "riot" }, { id: "epic" }],
    getUnavailablePlatformIds: () => state.unavailable,
    getActiveTab: () => state.activeTab,
    changeTab: vi.fn(),
    zoomIn: vi.fn(),
    zoomOut: vi.fn(),
    resetZoom: vi.fn(),
    getCurrentFolderId: () => state.folderId,
    navigateBack: vi.fn(),
    activateFocusedCard: vi.fn(() => true),
    renameFocusedCard: vi.fn(() => true),
    openFocusedCardContextMenu: vi.fn(() => true),
    toggleBulkEditAccount: vi.fn(),
    bulkEditSelectAll: vi.fn(),
    bulkEditDeselectAll: vi.fn(),
    ...overrides,
  };
  createKeyboardController({
    getScope: () => state.scope,
    isMac: () => false,
    bindings: createKeyboardBindings(deps),
  }).attach();
  function press(event: ReturnType<typeof key>) {
    listener?.(event as unknown as KeyboardEvent);
    return event.handled;
  }
  return { deps, state, focus, press };
}

describe("app keyboard bindings", () => {
  it("swallows the WebView built-ins in every scope", () => {
    const { state, press } = setup();
    for (const scope of ["locked", "dialog", "settings", "app"] as KeyScope[]) {
      state.scope = scope;
      expect(press(key("w", { ctrl: true }))).toBe(true);
      expect(press(key("p", { ctrl: true }))).toBe(true);
      expect(press(key("F3"))).toBe(true);
      expect(press(key("F5"))).toBe(true);
      expect(press(key("f", { ctrl: true }))).toBe(true);
      expect(press(key("ArrowRight", { alt: true }))).toBe(true);
    }
  });

  it("opens the palette over a context menu and closes it from inside", () => {
    const { deps, state, press } = setup();
    state.scope = "context-menu";
    expect(press(key("k", { ctrl: true }))).toBe(true);
    expect(deps.closeContextMenu).toHaveBeenCalledOnce();
    expect(deps.setPaletteOpen).toHaveBeenLastCalledWith(true);

    state.scope = "palette";
    press(key("k", { ctrl: true }));
    expect(deps.setPaletteOpen).toHaveBeenLastCalledWith(false);
  });

  it("cancels a drag before any other Escape layer", () => {
    const { deps, state, press } = setup({ cancelDragFromEscape: vi.fn(() => true) });
    state.scope = "palette";
    expect(press(key("Escape"))).toBe(true);
    expect(deps.setPaletteOpen).not.toHaveBeenCalled();
  });

  it("closes exactly one Escape layer and passes where the owner handles it", () => {
    const { deps, state, press } = setup();

    state.scope = "palette";
    expect(press(key("Escape"))).toBe(true);
    expect(deps.setPaletteOpen).toHaveBeenLastCalledWith(false);

    state.scope = "dialog";
    press(key("Escape"));
    expect(deps.closeConfirmDialog).toHaveBeenCalledOnce();
    vi.mocked(deps.hasInputDialog).mockReturnValue(true);
    press(key("Escape"));
    expect(deps.closeInputDialog).toHaveBeenCalledOnce();
    expect(deps.closeConfirmDialog).toHaveBeenCalledOnce();

    for (const scope of [
      "context-menu",
      "bulk-edit",
      "settings",
      "onboarding",
      "locked",
    ] as KeyScope[]) {
      state.scope = scope;
      expect(press(key("Escape")), scope).toBe(false);
    }
  });

  it("lets the persona wizard consume Escape before closing the panel", () => {
    const { deps, state, press } = setup();
    state.scope = "personas";
    vi.mocked(deps.personasPanelEscape).mockReturnValueOnce(true);
    expect(press(key("Escape"))).toBe(true);
    expect(deps.closePersonas).not.toHaveBeenCalled();

    expect(press(key("Escape"))).toBe(true);
    expect(deps.closePersonas).toHaveBeenCalledOnce();
  });

  it("clears the search, then the card focus, then passes Escape in the app", () => {
    const { deps, state, focus, press } = setup();
    const input = new FakeHTMLInputElement();
    state.searchInput = input;
    state.activeElement = input;
    expect(press(key("Escape", { target: input }))).toBe(true);
    expect(deps.clearSearchQuery).toHaveBeenCalledOnce();
    expect(input.blur).toHaveBeenCalledOnce();

    state.activeElement = null;
    focus.focusedId = "a1";
    expect(press(key("Escape"))).toBe(true);
    expect(focus.focusedId).toBeNull();

    expect(press(key("Escape"))).toBe(false);
  });

  it("routes mod+f to the grid search or the settings search", () => {
    const { deps, state, press } = setup();
    const input = new FakeHTMLInputElement();
    state.searchInput = input;
    press(key("f", { ctrl: true }));
    expect(input.focus).toHaveBeenCalledOnce();
    expect(input.select).toHaveBeenCalledOnce();

    state.scope = "settings";
    press(key("f", { ctrl: true }));
    expect(deps.focusSettingsSearch).toHaveBeenCalledOnce();
  });

  it("maps the app shortcuts to their actions", () => {
    const { deps, state, press } = setup();
    press(key("n", { ctrl: true }));
    expect(deps.addAccount).toHaveBeenCalledOnce();
    press(key("N", { ctrl: true, shift: true }));
    expect(deps.newFolder).toHaveBeenCalledOnce();
    press(key("r", { ctrl: true }));
    press(key("F5"));
    expect(deps.refresh).toHaveBeenCalledTimes(2);
    press(key("e", { ctrl: true }));
    expect(deps.toggleBulkEdit).toHaveBeenCalledOnce();
    press(key(",", { ctrl: true }));
    expect(deps.closePersonas).toHaveBeenCalledOnce();
    expect(deps.toggleSettingsPanel).toHaveBeenCalledOnce();
    press(key("P", { ctrl: true, shift: true }));
    expect(deps.openPersonas).toHaveBeenCalledOnce();
    press(key("L", { ctrl: true, shift: true }));
    expect(deps.toggleViewMode).toHaveBeenCalledOnce();
    press(key("=", { ctrl: true, code: "Equal" }));
    press(key("+", { ctrl: true, shift: true, code: "Equal" }));
    expect(deps.zoomIn).toHaveBeenCalledTimes(2);
    press(key("-", { ctrl: true, code: "Minus" }));
    expect(deps.zoomOut).toHaveBeenCalledOnce();
    press(key("0", { ctrl: true, code: "Digit0" }));
    expect(deps.resetZoom).toHaveBeenCalledOnce();

    // Mod+E is swallowed but does nothing while bulk edit is unavailable.
    vi.mocked(deps.isBulkEditToggleAllowed).mockReturnValue(false);
    expect(press(key("e", { ctrl: true }))).toBe(true);
    expect(deps.toggleBulkEdit).toHaveBeenCalledOnce();

    // Refresh by F5 only in the app scope; elsewhere it is only swallowed.
    state.scope = "settings";
    expect(press(key("F5"))).toBe(true);
    expect(deps.refresh).toHaveBeenCalledTimes(2);
  });

  it("switches tabs by digit and cycles past unavailable platforms", () => {
    const { deps, state, press } = setup();
    press(key("2", { ctrl: true, code: "Digit2" }));
    expect(deps.changeTab).toHaveBeenLastCalledWith("riot");
    expect(deps.closePersonas).toHaveBeenCalledOnce();

    // A missing or unavailable slot is handled but changes nothing.
    state.unavailable = new Set(["riot"]);
    expect(press(key("2", { ctrl: true, code: "Digit2" }))).toBe(true);
    expect(press(key("9", { ctrl: true, code: "Digit9" }))).toBe(true);
    expect(deps.changeTab).toHaveBeenCalledOnce();

    press(key("Tab", { ctrl: true }));
    expect(deps.changeTab).toHaveBeenLastCalledWith("epic");
    press(key("Tab", { ctrl: true, shift: true }));
    expect(deps.changeTab).toHaveBeenLastCalledWith("epic");
  });

  it("goes up a folder on Backspace and Alt+Left, and passes at the root", () => {
    const { deps, state, press } = setup();
    expect(press(key("Backspace"))).toBe(false);
    expect(press(key("ArrowLeft", { alt: true }))).toBe(true);
    expect(deps.navigateBack).not.toHaveBeenCalled();

    state.folderId = "f1";
    expect(press(key("Backspace"))).toBe(true);
    expect(press(key("ArrowLeft", { alt: true }))).toBe(true);
    expect(deps.navigateBack).toHaveBeenCalledTimes(2);

    // Outside the app scope only the catch-all PASS binding matches.
    state.scope = "settings";
    expect(press(key("ArrowLeft", { alt: true }))).toBe(false);
  });

  it("moves and activates the card focus, passing when nothing takes the key", () => {
    const { deps, focus, press } = setup();
    expect(CARD_NAV_SCOPES).toEqual(["app", "bulk-edit"]);
    expect(press(key("ArrowDown"))).toBe(true);
    focus.moves = false;
    expect(press(key("ArrowDown"))).toBe(false);

    expect(press(key("Enter"))).toBe(true);
    expect(deps.activateFocusedCard).toHaveBeenCalledOnce();
    focus.ownsKey = false;
    expect(press(key("Enter"))).toBe(false);
    expect(deps.activateFocusedCard).toHaveBeenCalledOnce();

    expect(press(key("F2"))).toBe(true);
    expect(press(key("Delete"))).toBe(true);
    expect(press(key("F10", { shift: true }))).toBe(true);
    expect(press(key("ContextMenu"))).toBe(true);
    expect(deps.openFocusedCardContextMenu).toHaveBeenCalledTimes(3);
    vi.mocked(deps.renameFocusedCard).mockReturnValue(false);
    expect(press(key("F2"))).toBe(false);
  });

  it("toggles the focused account with Space in bulk edit only", () => {
    const { deps, state, focus, press } = setup();
    state.scope = "bulk-edit";
    expect(press(key(" "))).toBe(false);
    focus.focusedItem = { type: "folder", id: "f1" };
    expect(press(key(" "))).toBe(false);
    focus.focusedItem = { type: "account", id: "a1" };
    expect(press(key(" "))).toBe(true);
    expect(deps.toggleBulkEditAccount).toHaveBeenCalledWith("a1");

    state.scope = "app";
    expect(press(key(" "))).toBe(false);
  });

  it("selects all in bulk edit, but not from inside a text field", () => {
    const { deps, state, press } = setup();
    state.scope = "bulk-edit";
    press(key("a", { ctrl: true }));
    press(key("d", { ctrl: true }));
    expect(deps.bulkEditSelectAll).toHaveBeenCalledOnce();
    expect(deps.bulkEditDeselectAll).toHaveBeenCalledOnce();

    const input = new FakeHTMLInputElement();
    expect(press(key("a", { ctrl: true, target: input }))).toBe(false);
    expect(deps.bulkEditSelectAll).toHaveBeenCalledOnce();
  });
});

describe("key scope", () => {
  function resolver(active: Partial<Record<string, boolean>>) {
    const read: string[] = [];
    const flag = (name: string) => () => {
      read.push(name);
      return active[name] ?? false;
    };
    const scope = createKeyScopeResolver({
      isLocked: flag("locked"),
      isPaletteOpen: flag("palette"),
      isOnboardingOpen: flag("onboarding"),
      hasDialog: flag("dialog"),
      hasContextMenu: flag("context-menu"),
      isBulkEditMode: flag("bulk-edit"),
      isSettingsOpen: flag("settings"),
      isPersonasOpen: flag("personas"),
    });
    return { scope, read };
  }

  it("picks the topmost layer and stops reading there", () => {
    expect(resolver({}).scope()).toBe("app");
    expect(resolver({ personas: true, settings: true }).scope()).toBe("settings");
    expect(resolver({ "bulk-edit": true, "context-menu": true }).scope()).toBe("context-menu");
    expect(resolver({ dialog: true, onboarding: true }).scope()).toBe("onboarding");
    expect(resolver({ locked: true, palette: true }).scope()).toBe("locked");

    const { scope, read } = resolver({ dialog: true });
    expect(scope()).toBe("dialog");
    expect(read).toEqual(["locked", "palette", "onboarding", "dialog"]);
  });
});
