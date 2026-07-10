import { invoke } from "@tauri-apps/api/core";
import { getPlatform } from "$lib/shared/platform";
import type { Persona } from "$lib/features/personas/types";
import {
  getPersonas,
  createPersona as createPersonaStore,
  updatePersona as updatePersonaStore,
  deletePersona as deletePersonaStore,
} from "$lib/features/personas/store";

export interface PersonaSwitchResult {
  succeeded: string[];
  failed: { platformId: string; error: string }[];
}

export function createPersonaController() {
  let personas = $state<Persona[]>(getPersonas());
  // Which persona is mid-switch, so the UI can lock and show progress.
  let switchingPersonaId = $state<string | null>(null);

  function refresh() {
    personas = getPersonas();
  }

  function create(input: Omit<Persona, "id">): Persona {
    const persona = createPersonaStore(input);
    refresh();
    return persona;
  }

  function update(id: string, patch: Partial<Omit<Persona, "id">>) {
    updatePersonaStore(id, patch);
    refresh();
  }

  function remove(id: string) {
    deletePersonaStore(id);
    refresh();
  }

  /**
   * Switch every assigned platform to the persona's account, one platform at a
   * time. Each platform switch closes and relaunches its client and takes the
   * backend's exclusive lock, so these must run sequentially, not in parallel.
   * A failure on one platform doesn't abort the rest; the caller reports which
   * platforms landed and which didn't.
   */
  async function switchToPersona(persona: Persona): Promise<PersonaSwitchResult | null> {
    if (switchingPersonaId) return null;
    switchingPersonaId = persona.id;
    const result: PersonaSwitchResult = { succeeded: [], failed: [] };
    try {
      for (const { platformId, accountId } of persona.assignments) {
        try {
          // Go through the platform adapter, not the raw backend command: some
          // adapters switch by a key that differs from the account id (Steam
          // switches by account_name while its id is the steam_id). Writing the
          // id verbatim would corrupt the platform's autologin state.
          const adapter = getPlatform(platformId);
          const account = adapter
            ? (await adapter.loadAccounts()).find((a) => a.id === accountId)
            : undefined;
          if (adapter && account) {
            await adapter.switchAccount(account);
          } else {
            await invoke("platform_switch_account", { platformId, accountId, params: {} });
          }
          result.succeeded.push(platformId);
        } catch (e) {
          result.failed.push({ platformId, error: String(e) });
        }
      }
    } finally {
      switchingPersonaId = null;
    }
    return result;
  }

  return {
    get personas() {
      return personas;
    },
    get switchingPersonaId() {
      return switchingPersonaId;
    },
    refresh,
    create,
    update,
    remove,
    switchToPersona,
  };
}
