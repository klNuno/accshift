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
