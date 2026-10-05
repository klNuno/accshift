import { describe, expect, it, vi } from "vitest";
import { takeNativeUnlisteners } from "./useWindowActivity.svelte";

describe("takeNativeUnlisteners", () => {
  it("disposes listeners that arrive after stop", () => {
    const focus = vi.fn();
    const resize = vi.fn();
    expect(takeNativeUnlisteners(false, [focus, resize])).toEqual([]);
    expect(focus).toHaveBeenCalledOnce();
    expect(resize).toHaveBeenCalledOnce();
  });

  it("keeps listeners while the controller is still started and skips a rejected sibling", () => {
    const focus = vi.fn();
    const resize = vi.fn();
    expect(takeNativeUnlisteners(true, [focus, undefined, resize])).toEqual([focus, resize]);
    expect(focus).not.toHaveBeenCalled();
    expect(resize).not.toHaveBeenCalled();
  });
});
