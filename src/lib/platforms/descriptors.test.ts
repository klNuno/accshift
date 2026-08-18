import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const {
  installDescriptorFile,
  reloadUserPlatforms,
  removeDescriptor,
  selectDescriptorFile,
  previewDescriptorFile,
} = await import("./descriptors");
const { getPlatformDefinition, registerUserPlatforms } = await import("./registry");

const ACME = {
  id: "acme",
  name: "Acme Launcher",
  os: { windows: {} },
};

function report(loaded: (typeof ACME)[]) {
  return { dir: "C:/data/platforms", loaded, skipped: [], rejected: [] };
}

beforeEach(() => {
  invokeMock.mockReset();
});

afterEach(() => {
  registerUserPlatforms([]);
});

describe("descriptor folder calls", () => {
  it("puts what the folder now holds into the platform list", async () => {
    invokeMock.mockResolvedValue(report([ACME]));

    const result = await reloadUserPlatforms();

    expect(invokeMock).toHaveBeenCalledWith("reload_user_platforms");
    expect(result.dir).toBe("C:/data/platforms");
    expect(getPlatformDefinition("acme")?.userProvided).toBe(true);
  });

  it("drops a platform whose file the user just removed", async () => {
    invokeMock.mockResolvedValueOnce(report([ACME])).mockResolvedValueOnce(report([]));
    await reloadUserPlatforms();

    await removeDescriptor("acme");

    expect(invokeMock).toHaveBeenLastCalledWith("descriptor_remove", { platformId: "acme" });
    expect(getPlatformDefinition("acme")).toBeUndefined();
  });

  it("registers what installing a file added, without a second round trip", async () => {
    invokeMock.mockResolvedValue(report([ACME]));

    await installDescriptorFile("C:/downloads/acme.json");

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(getPlatformDefinition("acme")?.name).toBe("Acme Launcher");
  });

  it("reads a cancelled file picker as nothing to do, not as a failure", async () => {
    invokeMock.mockRejectedValue("cancelled");

    await expect(selectDescriptorFile()).resolves.toBeNull();
  });

  it("lets a rejected descriptor reach the caller so it can say why", async () => {
    invokeMock.mockRejectedValue("acme.json: schemaVersion: expected 1");

    await expect(previewDescriptorFile("C:/downloads/acme.json")).rejects.toBe(
      "acme.json: schemaVersion: expected 1",
    );
  });
});
