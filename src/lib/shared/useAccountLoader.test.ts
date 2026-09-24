import { describe, expect, it, vi } from "vitest";
import type { PlatformAdapter, PlatformAddAccountResult } from "./platform";
import { createAccountLoader } from "./useAccountLoader.svelte";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("account add flow", () => {
  it("coalesces duplicate add requests while setup is starting", async () => {
    const pending = deferred<PlatformAddAccountResult>();
    const adapter = {
      id: "steam",
      addAccount: vi.fn().mockReturnValue(pending.promise),
    } as unknown as PlatformAdapter;
    const loader = createAccountLoader(() => adapter);

    const first = loader.addNew();
    const duplicate = loader.addNew();

    expect(loader.adding).toBe(true);
    expect(adapter.addAccount).toHaveBeenCalledOnce();
    await duplicate;

    pending.resolve({
      setupStatus: { setupId: "setup-1", state: "waiting_for_login" },
    });
    await first;

    expect(loader.adding).toBe(false);
    expect(adapter.addAccount).toHaveBeenCalledOnce();
  });
});

describe("load and switch overlap", () => {
  const alice = { id: "alice", username: "alice", displayName: "Alice" };
  const bob = { id: "bob", username: "bob", displayName: "Bob" };

  function adapterWithPendingSnapshot() {
    const snapshot = deferred<{ accounts: (typeof alice)[]; currentAccount: string }>();
    const adapter = {
      id: "riot",
      getStartupSnapshot: vi.fn().mockReturnValue(snapshot.promise),
      switchAccount: vi.fn().mockResolvedValue(undefined),
    } as unknown as PlatformAdapter;
    return { adapter, snapshot };
  }

  it("clears loading when a switch lands while a load is in flight", async () => {
    const { adapter, snapshot } = adapterWithPendingSnapshot();
    const loader = createAccountLoader(() => adapter);

    const load = loader.load();
    expect(loader.loading).toBe(true);
    await loader.switchTo(bob);
    snapshot.resolve({ accounts: [alice, bob], currentAccount: "alice" });
    await load;

    expect(loader.loading).toBe(false);
    expect(loader.accounts).toEqual([alice, bob]);
  });

  it("keeps the switched account over a snapshot read before the switch", async () => {
    const { adapter, snapshot } = adapterWithPendingSnapshot();
    const loader = createAccountLoader(() => adapter);

    const load = loader.load();
    await loader.switchTo(bob);
    snapshot.resolve({ accounts: [alice, bob], currentAccount: "alice" });
    await load;

    expect(loader.currentAccount).toBe("bob");
  });

  it("still lets a newer load supersede an older one", async () => {
    const first = deferred<{ accounts: (typeof alice)[]; currentAccount: string }>();
    const second = deferred<{ accounts: (typeof alice)[]; currentAccount: string }>();
    const adapter = {
      id: "riot",
      getStartupSnapshot: vi
        .fn()
        .mockReturnValueOnce(first.promise)
        .mockReturnValueOnce(second.promise),
    } as unknown as PlatformAdapter;
    const loader = createAccountLoader(() => adapter);

    const older = loader.load();
    const newer = loader.load();
    second.resolve({ accounts: [bob], currentAccount: "bob" });
    await newer;
    first.resolve({ accounts: [alice], currentAccount: "alice" });
    await older;

    expect(loader.accounts).toEqual([bob]);
    expect(loader.currentAccount).toBe("bob");
    expect(loader.loading).toBe(false);
  });
});
