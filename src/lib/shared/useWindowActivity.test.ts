import { describe, expect, it, vi } from "vitest";

type Registration = { resolve: (unlisten: () => void) => void };
const registrations: Registration[] = [];
function pendingRegistration() {
  return new Promise<() => void>((resolve) => registrations.push({ resolve }));
}

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onFocusChanged: pendingRegistration,
    onResized: pendingRegistration,
    isFocused: async () => true,
    isMinimized: async () => false,
  }),
}));

const { createWindowActivity, takeNativeUnlisteners } = await import("./useWindowActivity.svelte");

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

describe("createWindowActivity", () => {
  it("drops the native listeners of a pass stopped before they registered", async () => {
    registrations.length = 0;
    // The controller only reaches the native window where a DOM window exists.
    vi.stubGlobal("window", { addEventListener: vi.fn(), removeEventListener: vi.fn() });
    const activity = createWindowActivity();
    const first = activity.start();
    activity.stop();
    const second = activity.start();
    expect(registrations).toHaveLength(4);

    const unlistens = registrations.map(() => vi.fn());
    registrations.forEach((registration, index) => registration.resolve(unlistens[index]!));
    await Promise.all([first, second]);

    // The first pass's focus and resize listeners are removed at once.
    expect(unlistens[0]).toHaveBeenCalledOnce();
    expect(unlistens[1]).toHaveBeenCalledOnce();
    expect(unlistens[2]).not.toHaveBeenCalled();
    expect(unlistens[3]).not.toHaveBeenCalled();

    activity.stop();
    expect(unlistens[2]).toHaveBeenCalledOnce();
    expect(unlistens[3]).toHaveBeenCalledOnce();
    vi.unstubAllGlobals();
  });
});
