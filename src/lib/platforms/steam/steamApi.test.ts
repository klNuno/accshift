import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(false));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { hasApiKey, setApiKey } from "./steamApi";

function hasKeyCalls() {
  return invokeMock.mock.calls.filter((call) => call[0] === "steam_has_api_key");
}

describe("hasApiKey memo", () => {
  beforeEach(async () => {
    invokeMock.mockClear();
    invokeMock.mockImplementation(() => Promise.resolve(false));
    // The memo is module state: drop it through the public API so each test
    // starts undecided.
    await setApiKey("__test__");
    invokeMock.mockClear();
  });

  it("decrypts once then serves the memo", async () => {
    invokeMock.mockImplementationOnce(() => Promise.resolve(true));
    await expect(hasApiKey()).resolves.toBe(true);
    await expect(hasApiKey()).resolves.toBe(true);
    expect(hasKeyCalls()).toHaveLength(1);
  });

  it("setApiKey drops the memo so the next read decrypts fresh", async () => {
    invokeMock.mockImplementationOnce(() => Promise.resolve(false));
    await expect(hasApiKey()).resolves.toBe(false);
    await setApiKey("new-key");
    invokeMock.mockImplementationOnce(() => Promise.resolve(true));
    await expect(hasApiKey()).resolves.toBe(true);
    expect(hasKeyCalls()).toHaveLength(2);
  });

  it("a read in flight during setApiKey does not restore its answer", async () => {
    let settle: ((value: boolean) => void) | undefined;
    invokeMock.mockImplementationOnce(
      () =>
        new Promise<boolean>((resolve) => {
          settle = resolve;
        }),
    );
    const inFlight = hasApiKey();
    await setApiKey("new-key");
    settle?.(false);
    await expect(inFlight).resolves.toBe(false);

    invokeMock.mockImplementationOnce(() => Promise.resolve(true));
    await expect(hasApiKey()).resolves.toBe(true);
  });

  it("a failed setApiKey keeps the previous memo", async () => {
    invokeMock.mockImplementationOnce(() => Promise.resolve(true));
    await expect(hasApiKey()).resolves.toBe(true);
    invokeMock.mockImplementationOnce(() => Promise.reject(new Error("locked")));
    await expect(setApiKey("x")).rejects.toThrow();
    await expect(hasApiKey()).resolves.toBe(true);
    expect(hasKeyCalls()).toHaveLength(1);
  });
});
