import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ItemRef } from "$lib/features/folders/types";

// Vitest runs in node: a small Element stand-in with the three calls the
// helpers use (contains, matches, ownerDocument).
class FakeElement {
  children: FakeElement[] = [];
  parent: FakeElement | null = null;
  ownerDocument: { body: FakeElement | null; documentElement: FakeElement | null } = fakeDocument;
  constructor(
    public tagName: string,
    public attrs: Record<string, string> = {},
  ) {}
  append(child: FakeElement) {
    child.parent = this;
    this.children.push(child);
    return child;
  }
  contains(other: FakeElement | null): boolean {
    for (let node = other; node; node = node.parent) if (node === this) return true;
    return false;
  }
  matches(selector: string): boolean {
    return selector.split(",").some((raw) => {
      const part = raw.trim();
      if (part === this.tagName.toLowerCase()) return true;
      const attr = part.match(/^\[([\w-]+)(?:="([^"]*)")?\]/);
      if (!attr) return false;
      const value = this.attrs[attr[1]];
      if (value === undefined) return false;
      return attr[2] === undefined || attr[2] === value;
    });
  }
  querySelector() {
    return null;
  }
  querySelectorAll() {
    return [];
  }
}

const fakeDocument: { body: FakeElement | null; documentElement: FakeElement | null } = {
  body: null,
  documentElement: null,
};

beforeEach(() => {
  vi.stubGlobal("Element", FakeElement);
  vi.stubGlobal("CSS", { escape: (value: string) => value });
  fakeDocument.body = new FakeElement("BODY");
  fakeDocument.documentElement = new FakeElement("HTML");
});

afterEach(() => {
  vi.unstubAllGlobals();
});

const { createCardFocus, isCardKeyTarget } = await import("./useCardFocus.svelte");

function buildPage() {
  const body = fakeDocument.body!;
  const titlebarButton = body.append(new FakeElement("BUTTON"));
  const grid = body.append(new FakeElement("DIV"));
  const card = grid.append(new FakeElement("DIV", { role: "button", "data-account-id": "a1" }));
  const cardButton = card.append(new FakeElement("BUTTON"));
  const plainInGrid = grid.append(new FakeElement("SPAN"));
  const backRow = grid.append(new FakeElement("DIV", { role: "button", "data-back-card": "true" }));
  return { body, titlebarButton, grid, card, cardButton, plainInGrid, backRow };
}

describe("card key target", () => {
  it("leaves Enter to a focused button outside the grid", () => {
    const page = buildPage();
    expect(
      isCardKeyTarget(page.titlebarButton as unknown as Element, page.grid as unknown as Element),
    ).toBe(false);
  });

  it("leaves Enter to a control inside a card", () => {
    const page = buildPage();
    expect(
      isCardKeyTarget(page.cardButton as unknown as Element, page.grid as unknown as Element),
    ).toBe(false);
    expect(
      isCardKeyTarget(page.backRow as unknown as Element, page.grid as unknown as Element),
    ).toBe(false);
  });

  it("claims Enter when nothing or only a card holds real focus", () => {
    const page = buildPage();
    const grid = page.grid as unknown as Element;
    expect(isCardKeyTarget(page.body as unknown as Element, grid)).toBe(true);
    expect(isCardKeyTarget(null, grid)).toBe(true);
    expect(isCardKeyTarget(page.card as unknown as Element, grid)).toBe(true);
    expect(isCardKeyTarget(page.plainInGrid as unknown as Element, grid)).toBe(true);
  });

  it("does not claim Enter when the grid is not mounted", () => {
    const page = buildPage();
    expect(isCardKeyTarget(page.plainInGrid as unknown as Element, null)).toBe(false);
  });
});

describe("virtual focus release", () => {
  function focusOver(grid: FakeElement) {
    const items: ItemRef[] = [{ type: "account", id: "a1" }];
    const focus = createCardFocus({
      getItems: () => items,
      getWrapperRef: () => grid as unknown as HTMLElement,
      getViewMode: () => "grid",
    });
    focus.move("right");
    expect(focus.focusedId).toBe("a1");
    return focus;
  }

  it("clears when real focus moves outside the grid", () => {
    const page = buildPage();
    const focus = focusOver(page.grid);
    focus.releaseIfOutside(page.titlebarButton as unknown as Element);
    expect(focus.focusedId).toBeNull();
  });

  it("keeps the virtual focus for focus inside the grid or on the page root", () => {
    const page = buildPage();
    const focus = focusOver(page.grid);
    focus.releaseIfOutside(page.card as unknown as Element);
    focus.releaseIfOutside(page.body as unknown as Element);
    expect(focus.focusedId).toBe("a1");
  });
});
