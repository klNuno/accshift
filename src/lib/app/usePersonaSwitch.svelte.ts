import { invoke } from "@tauri-apps/api/core";
import type { Persona } from "$lib/features/personas/types";
import type { AddToastOptions } from "$lib/features/notifications/store.svelte";
import type { MessageKey, TranslationParams } from "$lib/i18n";
import type { PlatformDef, RuntimeOs } from "$lib/shared/platform";
import type { AppSettings } from "$lib/features/settings/types";
import type { PersonaSwitchResult } from "./usePersonas.svelte";

type PersonaSwitchDeps = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  showToast: (message: string, options?: AddToastOptions) => unknown;
  shell: {
    readonly runtimeOs: RuntimeOs;
    readonly settings: AppSettings;
  };
  /** Every platform definition; the persona slots are the usable ones. */
  platforms: readonly PlatformDef[];
  personas: {
    readonly switching: boolean;
    switchToPersona: (persona: Persona) => Promise<PersonaSwitchResult | null>;
  };
  /** True while a regular account switch holds the platform clients. */
  isAccountSwitching: () => boolean;
  isPersonasEnabled: () => boolean;
  isSettingsOpen: () => boolean;
  closeSettingsPanel: () => void;
  closeBulkEdit: () => void;
  requestConfirm: (config: {
    title: string;
    message: string;
    confirmLabel?: string;
  }) => Promise<boolean>;
};

/**
 * The personas panel: whether it is shown, the platforms it offers as slots,
 * and the confirmed switch with its outcome toast. Create during component
 * init: it registers the effect that closes the panel when the feature is
 * turned off.
 */
export function createPersonaSwitch({
  t,
  showToast,
  shell,
  platforms,
  personas,
  isAccountSwitching,
  isPersonasEnabled,
  isSettingsOpen,
  closeSettingsPanel,
  closeBulkEdit,
  requestConfirm,
}: PersonaSwitchDeps) {
  let showPersonas = $state(false);

  // Enabled, implemented platforms usable on this OS, offered as persona slots.
  let personaPlatforms = $derived(
    platforms
      .filter(
        (p) =>
          p.implemented &&
          p.supportedOs.includes(shell.runtimeOs) &&
          shell.settings.enabledPlatforms.includes(p.id),
      )
      .map((p) => ({ id: p.id, name: p.name, accent: p.accent })),
  );

  function openPersonas() {
    if (!isPersonasEnabled()) return;
    if (isSettingsOpen()) closeSettingsPanel();
    closeBulkEdit();
    showPersonas = true;
  }

  // Close the personas panel if the feature gets disabled in settings.
  $effect(() => {
    if (!isPersonasEnabled() && showPersonas) showPersonas = false;
  });

  async function handleSwitchPersona(persona: Persona) {
    if (personas.switching || isAccountSwitching()) return;
    // Same safeguard as remote-triggered account switches: activating a
    // persona closes and relaunches several game clients, never do that on a
    // stray click.
    const confirmed = await requestConfirm({
      title: t("personas.switchConfirmTitle"),
      message: t("personas.switchConfirmMessage", {
        name: persona.name,
        count: persona.assignments.length,
      }),
      confirmLabel: t("personas.switchConfirmAction"),
    });
    if (!confirmed) return;
    const result = await personas.switchToPersona(persona);
    if (!result) return;
    // Usage counters only (how many platforms targeted / landed); the backend
    // drops the event unless telemetry is opted in.
    void invoke("telemetry_track_persona_switch", {
      platforms: persona.assignments.length,
      succeeded: result.succeeded.length,
    }).catch(() => {});
    const nameFor = (id: string) => personaPlatforms.find((p) => p.id === id)?.name ?? id;
    if (result.failed.length === 0) {
      showToast(t("personas.switched", { name: persona.name }), { type: "success" });
    } else if (result.succeeded.length === 0) {
      showToast(t("personas.switchFailed", { name: persona.name }), { type: "error" });
    } else {
      showToast(
        t("personas.switchPartial", {
          name: persona.name,
          failed: result.failed.map((f) => nameFor(f.platformId)).join(", "),
        }),
      );
    }
  }

  return {
    get showPersonas() {
      return showPersonas;
    },
    set showPersonas(value: boolean) {
      showPersonas = value;
    },
    get personaPlatforms() {
      return personaPlatforms;
    },
    openPersonas,
    handleSwitchPersona,
  };
}
