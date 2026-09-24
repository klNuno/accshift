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

describe("persona switch guard", () => {
  beforeEach(() => {
    mocks.ensurePlatformLoaded.mockReset();
  });

  const persona = {
    id: "persona-1",
    name: "Main",
    color: "",
    assignments: [{ platformId: "steam", accountId: "steam-id" }],
  };

  it("reports the switch as in flight until every platform is done", async () => {
    let finishSwitch!: () => void;
    const adapter = {
      loadAccounts: vi.fn().mockResolvedValue([{ id: "steam-id", username: "u" }]),
      switchAccount: vi.fn(
        () =>
          new Promise<void>((resolve) => {
            finishSwitch = resolve;
          }),
      ),
    } as unknown as PlatformAdapter;
    mocks.ensurePlatformLoaded.mockResolvedValue(adapter);
    const controller = createPersonaController();

    const pending = controller.switchToPersona(persona);
    expect(controller.switching).toBe(true);
    await vi.waitFor(() => expect(adapter.switchAccount).toHaveBeenCalledOnce());
    finishSwitch();
    await pending;

    expect(controller.switching).toBe(false);
  });

  it("refuses to start while an account switch is running", async () => {
    const controller = createPersonaController({ isBlocked: () => true });

    const result = await controller.switchToPersona(persona);

    expect(result).toBeNull();
    expect(mocks.ensurePlatformLoaded).not.toHaveBeenCalled();
    expect(controller.switching).toBe(false);
  });
});
