import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createKeyboardController, PASS } from "./controller";
import type { KeyScope, ShortcutBinding } from "./types";

// Vitest runs in node: stand in for the two DOM classes the controller checks.
class FakeHTMLElement {
  isContentEditable = false;
  constructor(public tagName: string) {}
}

type Listener = (e: KeyboardEvent) => void;

function fakeKeyEvent(
  key: string,
  opts: { ctrlKey?: boolean; shiftKey?: boolean; target?: unknown } = {},
) {
  const event = {
    key,
    code: "",
    ctrlKey: opts.ctrlKey ?? false,
    metaKey: false,
    shiftKey: opts.shiftKey ?? false,
    altKey: false,
    target: opts.target ?? null,
    defaultPrevented: false,
    propagationStopped: false,
    preventDefault() {
      event.defaultPrevented = true;
    },
    stopPropagation() {
      event.propagationStopped = true;
    },
  };
  return event;
}

let listener: Listener | null = null;

beforeEach(() => {
  listener = null;
  vi.stubGlobal("HTMLElement", FakeHTMLElement);
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

function mount(bindings: ShortcutBinding[], scope: KeyScope = "app") {
  const controller = createKeyboardController({
    getScope: () => scope,
    isMac: () => false,
    bindings,
  });
  controller.attach();
  return (event: ReturnType<typeof fakeKeyEvent>) => listener?.(event as unknown as KeyboardEvent);
}

describe("keyboard controller", () => {
  it("treats a binding that returns nothing as handled and stops the event", () => {
    const run = vi.fn();
    const press = mount([{ combo: "escape", scopes: ["app"], run }]);
    const event = fakeKeyEvent("Escape");

    press(event);

    expect(run).toHaveBeenCalledOnce();
    expect(event.defaultPrevented).toBe(true);
    expect(event.propagationStopped).toBe(true);
  });

  it("stops the event even when a binding's body evaluates to false", () => {
    // The palette bug: `run: () => (open = false)` returned false, which the
    // old contract read as "not handled" and let Escape close the panel below.
    let paletteOpen = true;
    const later = vi.fn();
    const press = mount(
      [
        {
          combo: "escape",
          scopes: ["app"],
          run: () => {
            paletteOpen = false;
          },
        },
        { combo: "escape", scopes: ["app"], run: later },
      ],
      "app",
    );
    const event = fakeKeyEvent("Escape");

    press(event);

    expect(paletteOpen).toBe(false);
    expect(later).not.toHaveBeenCalled();
    expect(event.propagationStopped).toBe(true);
  });

  it("falls through to the next binding and leaves the event alone on PASS", () => {
    const next = vi.fn((): typeof PASS => PASS);
    const press = mount([
      { combo: "escape", scopes: ["app"], run: () => PASS },
      { combo: "escape", scopes: ["app"], run: next },
    ]);
    const event = fakeKeyEvent("Escape");

    press(event);

    expect(next).toHaveBeenCalledOnce();
    expect(event.defaultPrevented).toBe(false);
    expect(event.propagationStopped).toBe(false);
  });

  it("runs modifier combos inside a text field unless the binding opts out", () => {
    const selectAll = vi.fn();
    const zoom = vi.fn();
    const press = mount(
      [
        { combo: "mod+a", scopes: ["bulk-edit"], skipInInput: true, run: selectAll },
        { combo: "mod+plus", scopes: ["*"], run: zoom },
      ],
      "bulk-edit",
    );
    const input = new FakeHTMLElement("INPUT");

    const selectEvent = fakeKeyEvent("a", { ctrlKey: true, target: input });
    press(selectEvent);
    expect(selectAll).not.toHaveBeenCalled();
    expect(selectEvent.defaultPrevented).toBe(false);

    const zoomEvent = { ...fakeKeyEvent("+", { ctrlKey: true, target: input }), code: "Equal" };
    press(zoomEvent);
    expect(zoom).toHaveBeenCalledOnce();
  });

  it("still runs skipInInput bindings when focus is not in a field", () => {
    const selectAll = vi.fn();
    const press = mount(
      [{ combo: "mod+a", scopes: ["bulk-edit"], skipInInput: true, run: selectAll }],
      "bulk-edit",
    );

    press(fakeKeyEvent("a", { ctrlKey: true, target: new FakeHTMLElement("BUTTON") }));

    expect(selectAll).toHaveBeenCalledOnce();
  });

  it("rejects a binding whose body is an assignment expression at compile time", () => {
    let open = true;
    const binding: ShortcutBinding = {
      combo: "escape",
      scopes: ["app"],
      // @ts-expect-error an expression body returns the assigned value; use a block body
      run: () => (open = false),
    };
    expect(binding.combo).toBe("escape");
    expect(open).toBe(true);
  });
});
