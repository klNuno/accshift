import { describe, it, expect, afterEach } from "vitest";
import {
  PLATFORM_DEFS,
  getPlatformDefinition,
  registerUserPlatforms,
  type PlatformDescriptor,
} from "./registry";
import { resolvePathPlaceholder } from "$lib/shared/platform";

const ACME: PlatformDescriptor = {
  id: "acme",
  name: "Acme Launcher",
  os: {
    windows: {
      executable: {
        candidates: [
          { kind: "registry" },
          { kind: "path", template: "${ProgramFiles}/Acme/Acme.exe" },
        ],
      },
    },
    linux: {},
  },
};

afterEach(() => {
  registerUserPlatforms([]);
});

describe("shipped platforms", () => {
  it("takes its description from the descriptor the engine runs", () => {
    // Nothing in the frontend repeats these; a change to gog.json lands here.
    const gog = getPlatformDefinition("gog");
    expect(gog?.name).toBe("GOG Galaxy");
    expect(gog?.supportedOs).toEqual(["windows"]);
    expect(resolvePathPlaceholder(gog?.pathPlaceholder, "windows")).toContain("GalaxyClient.exe");
  });

  it("keeps describing the platforms no descriptor covers", () => {
    const steam = getPlatformDefinition("steam");
    expect(steam?.name).toBe("Steam");
    expect(steam?.supportedOs).toEqual(["windows", "linux", "macos"]);
  });

  it("marks none of them as user provided", () => {
    expect(PLATFORM_DEFS.every((platform) => !platform.userProvided)).toBe(true);
  });
});

describe("user platforms", () => {
  it("becomes a platform with no entry written for it", () => {
    registerUserPlatforms([ACME]);

    const acme = getPlatformDefinition("acme");
    expect(acme?.name).toBe("Acme Launcher");
    expect(acme?.userProvided).toBe(true);
    expect(acme?.implemented).toBe(true);
    expect(acme?.supportedOs).toEqual(["windows", "linux"]);
    expect(acme?.accent).toMatch(/^hsl\(/);
  });

  it("spells the launcher path the way the system does", () => {
    registerUserPlatforms([ACME]);

    const acme = getPlatformDefinition("acme");
    expect(resolvePathPlaceholder(acme?.pathPlaceholder, "windows")).toBe(
      "%ProgramFiles%\\Acme\\Acme.exe",
    );
    // The Linux profile names no place to look, so it carries no hint of its
    // own rather than borrowing the Windows one.
    expect(acme?.pathPlaceholder).toEqual({ windows: "%ProgramFiles%\\Acme\\Acme.exe" });
  });

  it("disappears when its file does", () => {
    registerUserPlatforms([ACME]);
    registerUserPlatforms([]);

    expect(getPlatformDefinition("acme")).toBeUndefined();
    expect(getPlatformDefinition("gog")?.name).toBe("GOG Galaxy");
  });

  it("cannot take over a platform this build ships", () => {
    registerUserPlatforms([{ ...ACME, id: "steam", name: "Not Steam" }]);

    expect(getPlatformDefinition("steam")?.name).toBe("Steam");
    expect(PLATFORM_DEFS.filter((platform) => platform.id === "steam")).toHaveLength(1);
  });

  it("keeps the same colour across reloads", () => {
    registerUserPlatforms([ACME]);
    const first = getPlatformDefinition("acme")?.accent;
    registerUserPlatforms([]);
    registerUserPlatforms([ACME]);

    expect(getPlatformDefinition("acme")?.accent).toBe(first);
  });
});
