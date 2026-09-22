import { describe, expect, it } from "vitest";
import { addToast, getToasts, removeToast, resolveToastDuration } from "./store.svelte";

/** Empties the module level store so each case starts from nothing. */
function drain() {
  while (getToasts().length > 0) removeToast(getToasts()[0].id);
}

describe("toast duration rules", () => {
  it("never gives an error toast a timer", () => {
    expect(resolveToastDuration("error")).toBeNull();
  });

  it("ignores an explicit duration on an error toast", () => {
    expect(resolveToastDuration("error", 1500)).toBeNull();
  });

  it("keeps the default timer on the other types", () => {
    expect(resolveToastDuration("info")).toBe(3000);
    expect(resolveToastDuration("success")).toBe(3000);
  });

  it("honours an explicit duration on a non error toast", () => {
    expect(resolveToastDuration("info", 1000)).toBe(1000);
    expect(resolveToastDuration("info", null)).toBeNull();
  });
});

describe("toast store", () => {
  it("stores an error toast with no duration", () => {
    drain();
    const id = addToast("boom", { type: "error" });
    expect(getToasts().find((toast) => toast.id === id)?.durationMs).toBeNull();
    drain();
  });

  it("drops the timer when a message is re-raised as an error", () => {
    drain();
    addToast("same text");
    const id = addToast("same text", { type: "error" });
    const toast = getToasts().find((entry) => entry.id === id);
    expect(getToasts()).toHaveLength(1);
    expect(toast?.type).toBe("error");
    expect(toast?.durationMs).toBeNull();
    // The component restarts on resetKey, so the dedup has to bump it.
    expect(toast?.resetKey).toBe(1);
    drain();
  });
});
