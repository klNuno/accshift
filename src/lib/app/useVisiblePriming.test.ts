import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createVisiblePriming, VISIBLE_PRIME_TTL_MS } from "./useVisiblePriming.svelte";

describe("visible priming", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function priming() {
    const primed: string[][] = [];
    const controller = createVisiblePriming({
      prepareAccountIds: () => 0,
      primeAccountIds: async (ids) => {
        primed.push([...ids]);
      },
      now: () => Date.now(),
    });
    return { controller, primed };
  }

  it("does not prime the same accounts again after a pause within the TTL", () => {
    const { controller, primed } = priming();
    controller.processVisible(["a", "b"], "steam", false);
    vi.advanceTimersByTime(200);
    expect(primed).toEqual([["a", "b"]]);

    // Alt-tab away and back.
    controller.pause();
    controller.processVisible(["a", "b"], "steam", false);
    vi.advanceTimersByTime(200);
    expect(primed).toHaveLength(1);

    controller.processVisible(["a", "b", "c"], "steam", false);
    vi.advanceTimersByTime(200);
    expect(primed[1]).toEqual(["c"]);
  });

  it("primes again once the TTL has passed, and after a reset", () => {
    const { controller, primed } = priming();
    controller.processVisible(["a"], "steam", false);
    vi.advanceTimersByTime(200);

    controller.pause();
    vi.advanceTimersByTime(VISIBLE_PRIME_TTL_MS + 1);
    controller.processVisible(["a"], "steam", false);
    vi.advanceTimersByTime(200);
    expect(primed).toEqual([["a"], ["a"]]);

    controller.reset();
    controller.processVisible(["a"], "steam", false);
    vi.advanceTimersByTime(200);
    expect(primed).toHaveLength(3);
  });
});
