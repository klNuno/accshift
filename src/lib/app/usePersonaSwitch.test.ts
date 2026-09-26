import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn(async (_command: string, _args?: unknown) => {});
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));

import type { Persona } from "$lib/features/personas/types";
import type { AppSettings } from "$lib/features/settings/types";
import type { PlatformDef } from "$lib/shared/platform";
import type { PersonaSwitchResult } from "./usePersonas.svelte";
import { createPersonaSwitch } from "./usePersonaSwitch.svelte";

const platforms = [
  { id: "steam", name: "Steam", accent: "#1", implemented: true, supportedOs: ["windows"] },
  { id: "riot", name: "Riot", accent: "#2", implemented: true, supportedOs: ["windows"] },
  { id: "epic", name: "Epic", accent: "#3", implemented: false, supportedOs: ["windows"] },
] as unknown as PlatformDef[];

const persona = {
  id: "p1",
  name: "Main",
  assignments: [
    { platformId: "steam", accountId: "s1" },
    { platformId: "riot", accountId: "r1" },
  ],
} as unknown as Persona;

function setup(result: PersonaSwitchResult | null, opts: { confirmed?: boolean } = {}) {
  const toasts: [string, unknown][] = [];
  const calls: string[] = [];
  const switchToPersona = vi.fn(async () => result);
  const controller = createPersonaSwitch({
    t: (key, params) => (params ? `${key}:${JSON.stringify(params)}` : key),
    showToast: (message, options) => {
      toasts.push([message, options]);
    },
    shell: {
      runtimeOs: "windows",
      settings: { enabledPlatforms: ["steam", "riot", "epic"] } as unknown as AppSettings,
    },
    platforms,
    personas: { switching: false, switchToPersona },
    isAccountSwitching: () => false,
    isPersonasEnabled: () => true,
    isSettingsOpen: () => true,
    closeSettingsPanel: () => calls.push("closeSettings"),
    closeBulkEdit: () => calls.push("closeBulkEdit"),
    requestConfirm: async () => opts.confirmed ?? true,
  });
  return { controller, toasts, calls, switchToPersona };
}

describe("persona switch", () => {
  beforeEach(() => invoke.mockClear());

  it("offers the implemented, enabled platforms of this OS as slots", () => {
    const { controller } = setup(null);
    expect(controller.personaPlatforms).toEqual([
      { id: "steam", name: "Steam", accent: "#1" },
      { id: "riot", name: "Riot", accent: "#2" },
    ]);
  });

  it("opens the panel after closing settings and bulk edit", () => {
    const { controller, calls } = setup(null);
    controller.openPersonas();
    expect(calls).toEqual(["closeSettings", "closeBulkEdit"]);
    expect(controller.showPersonas).toBe(true);
  });

  it("does nothing without a confirmation", async () => {
    const { controller, switchToPersona, toasts } = setup(null, { confirmed: false });
    await controller.handleSwitchPersona(persona);
    expect(switchToPersona).not.toHaveBeenCalled();
    expect(toasts).toEqual([]);
  });

  it("toasts success, total failure and a partial switch by platform name", async () => {
    const ok = setup({ succeeded: ["steam", "riot"], failed: [] });
    await ok.controller.handleSwitchPersona(persona);
    expect(ok.toasts).toEqual([['personas.switched:{"name":"Main"}', { type: "success" }]]);
    expect(invoke).toHaveBeenCalledWith("telemetry_track_persona_switch", {
      platforms: 2,
      succeeded: 2,
    });

    const failed = setup({
      succeeded: [],
      failed: [
        { platformId: "steam", error: "x" },
        { platformId: "riot", error: "y" },
      ],
    });
    await failed.controller.handleSwitchPersona(persona);
    expect(failed.toasts).toEqual([['personas.switchFailed:{"name":"Main"}', { type: "error" }]]);

    const partial = setup({
      succeeded: ["steam"],
      failed: [
        { platformId: "riot", error: "y" },
        { platformId: "gog", error: "z" },
      ],
    });
    await partial.controller.handleSwitchPersona(persona);
    expect(partial.toasts).toEqual([
      ['personas.switchPartial:{"name":"Main","failed":"Riot, gog"}', undefined],
    ]);
  });
});
