import { afterEach, describe, expect, it, vi } from "vitest";
import type { PlatformAdapter, PlatformAddFlowStatus } from "$lib/shared/platform";

const mocks = vi.hoisted(() => ({
  adapter: undefined as PlatformAdapter | undefined,
  trackOperationFailed: vi.fn(),
}));

vi.mock("$lib/app/telemetryClient", () => ({
  trackAccountAdded: vi.fn(),
  trackAccountAddCancelled: vi.fn(),
  trackAccountAddStarted: vi.fn(),
  trackOperationFailed: (...args: unknown[]) => mocks.trackOperationFailed(...args),
}));

vi.mock("$lib/shared/platform", () => ({
  getPlatform: () => mocks.adapter,
}));

vi.mock("$lib/platforms/registry", () => ({
  getPlatformDefinition: () => undefined,
}));

import { createPlatformAddFlowController } from "./platformAddFlow.svelte";

function controller() {
  return createPlatformAddFlowController({
    getActiveTab: () => "steam",
    getCurrentFolderId: () => null,
    getIsSearching: () => false,
    // The key is the assertion target: no dictionary, no interpolation.
    t: (key) => key,
    showToast: vi.fn(),
    copyToClipboard: vi.fn(),
    loadAccounts: vi.fn(),
  });
}

const BUSY: PlatformAddFlowStatus = { setupId: "setup-1", state: "busy" };

afterEach(() => {
  mocks.adapter = undefined;
  mocks.trackOperationFailed.mockReset();
});

describe("a contended lock during setup", () => {
  it("says another operation is running instead of waiting for a login", () => {
    const flow = controller();
    flow.start("steam", BUSY);

    const content = flow.getSetupExtensionContent("setup-1");
    flow.stop();

    expect(content?.sections[0]?.text).toBe("platform.setupBusy");
    expect(content?.sections[0]?.loading).toBe(true);
  });

  it("labels the pending card with it too", () => {
    const flow = controller();
    flow.start("steam", BUSY);

    const pending = flow.pendingSetupAccount;
    flow.stop();

    expect(pending?.username).toBe("platform.setupBusy");
  });

  it("is non-terminal: the poll keeps the flow alive", async () => {
    mocks.adapter = {
      pollAddFlow: vi.fn().mockResolvedValue(BUSY),
    } as unknown as PlatformAdapter;
    const flow = controller();
    flow.start("steam", { setupId: "setup-1", state: "waiting_for_login" });

    await flow.poll();
    const state = flow.flow?.status.state;
    flow.stop();

    expect(state).toBe("busy");
    expect(mocks.trackOperationFailed).not.toHaveBeenCalled();
  });
});
