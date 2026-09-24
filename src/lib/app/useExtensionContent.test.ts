import { describe, expect, it } from "vitest";
import type { AccountWarningPresentation } from "$lib/shared/accountWarnings";
import { createExtensionContentController } from "./useExtensionContent.svelte";

// Vitest runs the server build of Svelte, where a derived recomputes on every
// read. Plain inputs are enough: what is under test is the per-account memo.
function setup() {
  const state = {
    warnings: {} as Record<string, AccountWarningPresentation>,
    notes: {} as Record<string, string>,
  };
  const controller = createExtensionContentController({
    t: (key) => key,
    getLocale: () => "en",
    getWarningStates: () => state.warnings,
    getVisibleRenderedAccountIds: () => ["a", "b"],
    getSetupExtensionContent: () => null,
    getAccountNote: (id) => state.notes[id] ?? "",
    getCardNoteVersion: () => 0,
    getShowCardNotesInline: () => false,
  });
  return { state, controller };
}

describe("extension content", () => {
  it("keeps the other cards' content when one account changes", () => {
    const { state, controller } = setup();
    state.notes = { a: "first", b: "second" };
    const before = controller.accountExtensionContentById;

    state.notes = { a: "first", b: "changed" };
    const after = controller.accountExtensionContentById;

    expect(after.a).toBe(before.a);
    expect(after.b).not.toBe(before.b);
    expect(after.b?.sections[0].lines).toEqual(["changed"]);
  });

  it("sees a chip text change even when the chip count stays the same", () => {
    const { state, controller } = setup();
    state.warnings = { a: { tooltipText: "", chips: [{ text: "VAC", tone: "danger" }] } as never };
    const before = controller.accountExtensionContentById.a;

    state.warnings = {
      a: { tooltipText: "", chips: [{ text: "Trade", tone: "danger" }] } as never,
    };
    const after = controller.accountExtensionContentById.a;

    expect(after).not.toBe(before);
  });
});
