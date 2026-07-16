import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PlatformAdapter } from "$lib/shared/platform";

const mocks = vi.hoisted(() => ({
  ensurePlatformLoaded: vi.fn(),
}));

vi.mock("$lib/platforms/registry", () => ({
  ensurePlatformLoaded: (...args: unknown[]) => mocks.ensurePlatformLoaded(...args),
}));

vi.mock("$lib/features/personas/store", () => ({
  getPersonas: () => [],
  createPersona: vi.fn(),
  updatePersona: vi.fn(),
  deletePersona: vi.fn(),
}));

import { createPersonaController } from "./usePersonas.svelte";

describe("persona adapter resolution", () => {
  beforeEach(() => {
    mocks.ensurePlatformLoaded.mockReset();
  });

  it("switches with the adapter's resolved account object", async () => {
    const account = { id: "steam-id", username: "login-name", displayName: "Player" };
    const adapter = {
      loadAccounts: vi.fn().mockResolvedValue([account]),
      switchAccount: vi.fn().mockResolvedValue(undefined),
    } as unknown as PlatformAdapter;
    mocks.ensurePlatformLoaded.mockResolvedValue(adapter);

    const result = await createPersonaController().switchToPersona({
      id: "persona-1",
      name: "Main",
      color: "",
      assignments: [{ platformId: "steam", accountId: "steam-id" }],
    });

    expect(adapter.switchAccount).toHaveBeenCalledWith(account);
    expect(result).toEqual({ succeeded: ["steam"], failed: [] });
  });

  it("fails the assignment instead of switching with an unresolved raw id", async () => {
    const adapter = {
      loadAccounts: vi.fn().mockResolvedValue([]),
      switchAccount: vi.fn(),
    } as unknown as PlatformAdapter;
    mocks.ensurePlatformLoaded.mockResolvedValue(adapter);

    const result = await createPersonaController().switchToPersona({
      id: "persona-1",
      name: "Main",
      color: "",
      assignments: [{ platformId: "steam", accountId: "steam-id" }],
    });

    expect(adapter.switchAccount).not.toHaveBeenCalled();
    expect(result?.succeeded).toEqual([]);
    expect(result?.failed[0]?.platformId).toBe("steam");
  });
});
