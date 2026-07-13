import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  fetch: vi.fn(),
  getSettings: vi.fn(),
}));

vi.mock("./steamApi", () => ({
  checkCs2Bridge: (...args: unknown[]) => mocks.check(...args),
  fetchCs2BridgeAccounts: (...args: unknown[]) => mocks.fetch(...args),
  getCs2BridgeSettings: (...args: unknown[]) => mocks.getSettings(...args),
}));

vi.mock("$lib/shared/appLogger", () => ({
  logAppEvent: vi.fn(() => Promise.resolve()),
  serializeLogValue: (value: unknown) => String(value),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function account(steamId: string) {
  return {
    steamId,
    level: 10,
    xp: 1200,
    xpMax: 5000,
    caseEarned: false,
    weekStartTs: null,
    lastUpdated: null,
  };
}

describe("CS2 bridge operation queue", () => {
  beforeEach(() => {
    vi.resetModules();
    mocks.check.mockReset();
    mocks.fetch.mockReset();
    mocks.getSettings.mockReset().mockResolvedValue({ enabled: true });
  });

  it("serializes rapid account checks in request order", async () => {
    const first = deferred<ReturnType<typeof account>>();
    const second = deferred<ReturnType<typeof account>>();
    mocks.check.mockImplementation((steamId: string) =>
      steamId === "one" ? first.promise : second.promise,
    );
    const bridge = await import("./cs2Bridge.svelte");

    const firstRun = bridge.triggerCs2BridgeCheck("one");
    const secondRun = bridge.triggerCs2BridgeCheck("two");

    await vi.waitFor(() => expect(mocks.check).toHaveBeenCalledTimes(1));
    expect(mocks.check).toHaveBeenNthCalledWith(1, "one");

    first.resolve(account("one"));
    await firstRun;
    await vi.waitFor(() => expect(mocks.check).toHaveBeenCalledTimes(2));
    expect(mocks.check).toHaveBeenNthCalledWith(2, "two");

    second.resolve(account("two"));
    await secondRun;
    expect(bridge.getCs2BridgeData("one")?.steamId).toBe("one");
    expect(bridge.getCs2BridgeData("two")?.steamId).toBe("two");
  });

  it("orders a check after an in-flight full fetch", async () => {
    const fetchResult = deferred<ReturnType<typeof account>[]>();
    mocks.fetch.mockReturnValue(fetchResult.promise);
    mocks.check.mockResolvedValue(account("fresh"));
    const bridge = await import("./cs2Bridge.svelte");

    const fetchRun = bridge.loadCs2BridgeData(true);
    const checkRun = bridge.triggerCs2BridgeCheck("fresh");

    await vi.waitFor(() => expect(mocks.fetch).toHaveBeenCalledTimes(1));
    expect(mocks.check).not.toHaveBeenCalled();

    fetchResult.resolve([account("stale")]);
    await fetchRun;
    await checkRun;

    expect(mocks.check).toHaveBeenCalledWith("fresh");
    expect(bridge.getCs2BridgeData("stale")?.steamId).toBe("stale");
    expect(bridge.getCs2BridgeData("fresh")?.steamId).toBe("fresh");
  });
});
