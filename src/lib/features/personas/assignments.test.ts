import { describe, expect, it } from "vitest";
import { personaAssignmentsForSave } from "./assignments";
import type { PersonaAssignment } from "./types";

const steam: PersonaAssignment = { platformId: "steam", accountId: "steam-1" };
const discord: PersonaAssignment = { platformId: "discord", accountId: "discord-1" };

function save(overrides: Partial<Parameters<typeof personaAssignmentsForSave>[0]> = {}) {
  return personaAssignmentsForSave({
    original: [steam, discord],
    offeredPlatformIds: ["steam"],
    selection: { steam: "steam-1" },
    accountsByPlatform: { steam: [{ id: "steam-1" }] },
    loading: false,
    failedPlatformIds: [],
    ...overrides,
  });
}

describe("personaAssignmentsForSave", () => {
  it("keeps an assignment for a platform the wizard is not offering", () => {
    expect(save()).toEqual([steam, discord]);
  });

  it("keeps an assignment when that platform's account read failed", () => {
    expect(
      save({
        offeredPlatformIds: ["steam", "discord"],
        selection: { steam: "steam-1", discord: "discord-1" },
        accountsByPlatform: { steam: [{ id: "steam-1" }], discord: [] },
        failedPlatformIds: ["discord"],
      }),
    ).toEqual([steam, discord]);
  });

  it("drops an assignment a successful read proved is gone", () => {
    expect(
      save({
        offeredPlatformIds: ["steam", "discord"],
        selection: { steam: "steam-1", discord: "discord-1" },
        accountsByPlatform: { steam: [{ id: "steam-1" }], discord: [] },
      }),
    ).toEqual([steam]);
  });

  it("drops a platform the user cleared and keeps the one they could not edit", () => {
    expect(save({ selection: { steam: "" } })).toEqual([discord]);
  });

  it("appends a newly picked platform after the original assignments", () => {
    const riot: PersonaAssignment = { platformId: "riot", accountId: "riot-1" };
    expect(
      save({
        offeredPlatformIds: ["steam", "riot"],
        selection: { steam: "steam-1", riot: "riot-1" },
        accountsByPlatform: {
          steam: [{ id: "steam-1" }],
          riot: [{ id: "riot-1" }],
        },
      }),
    ).toEqual([steam, discord, riot]);
  });
});
