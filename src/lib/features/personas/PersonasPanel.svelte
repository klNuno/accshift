<script lang="ts">
  import type { Persona } from "./types";
  import PersonaEditor from "./PersonaEditor.svelte";
  import type { PlatformAccount } from "$lib/shared/platform";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    personas,
    switchingPersonaId,
    platforms,
    loadAccounts,
    onSwitch,
    onCreate,
    onUpdate,
    onDelete,
    onClose,
    t,
  }: {
    personas: Persona[];
    switchingPersonaId: string | null;
    platforms: { id: string; name: string; accent: string }[];
    loadAccounts: (platformId: string) => Promise<PlatformAccount[]>;
    onSwitch: (persona: Persona) => void;
    onCreate: (input: Omit<Persona, "id">) => void;
    onUpdate: (id: string, patch: Partial<Omit<Persona, "id">>) => void;
    onDelete: (id: string) => void;
    onClose: () => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();

  // null = closed, "new" = create, otherwise the persona being edited
  let editing = $state<Persona | "new" | null>(null);

  function platformName(id: string): string {
    return platforms.find((p) => p.id === id)?.name ?? id;
  }
  function platformAccent(id: string): string {
    return platforms.find((p) => p.id === id)?.accent ?? "var(--fg-muted)";
  }

  function handleSave(input: Omit<Persona, "id">) {
    if (editing && editing !== "new") {
      onUpdate(editing.id, input);
    } else {
      onCreate(input);
    }
    editing = null;
  }

  function handleDelete() {
    if (editing && editing !== "new") {
      onDelete(editing.id);
    }
    editing = null;
  }
</script>

<div class="personas-panel">
  <header class="panel-head">
    <div>
      <h2>{t("personas.title")}</h2>
      <p class="subtitle">{t("personas.subtitle")}</p>
    </div>
    <button class="close-btn" onclick={onClose} aria-label={t("common.close")}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
        <path d="M18 6 6 18M6 6l12 12" />
      </svg>
    </button>
  </header>

  <div class="grid">
    {#each personas as persona (persona.id)}
      {@const busy = switchingPersonaId === persona.id}
      {@const anyBusy = switchingPersonaId !== null}
      <div class="card" style={`--persona-color:${persona.color};`}>
        <button
          class="card-main"
          disabled={anyBusy}
          onclick={() => onSwitch(persona)}
          title={t("personas.activate")}
        >
          <span class="avatar">{persona.emoji}</span>
          <span class="card-name">{persona.name}</span>
          <span class="card-platforms">
            {#each persona.assignments as a (a.platformId)}
              <span class="chip" style={`color:${platformAccent(a.platformId)};`}>
                {platformName(a.platformId)}
              </span>
            {/each}
            {#if persona.assignments.length === 0}
              <span class="chip empty">{t("personas.noPlatforms")}</span>
            {/if}
          </span>
          {#if busy}
            <span class="switching">{t("personas.switching")}</span>
          {/if}
        </button>
        <button class="edit-btn" disabled={anyBusy} onclick={() => (editing = persona)} aria-label={t("personas.edit")}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 20h9" />
            <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5z" />
          </svg>
        </button>
      </div>
    {/each}

    <button class="card new-card" onclick={() => (editing = "new")}>
      <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
        <path d="M12 5v14M5 12h14" />
      </svg>
      <span>{t("personas.create")}</span>
    </button>
  </div>

  {#if personas.length === 0}
    <p class="empty-hint">{t("personas.emptyHint")}</p>
  {/if}
</div>

{#if editing}
  <PersonaEditor
    persona={editing === "new" ? null : editing}
    {platforms}
    {loadAccounts}
    onSave={handleSave}
    onCancel={() => (editing = null)}
    onDelete={editing === "new" ? null : handleDelete}
    {t}
  />
{/if}

<style>
  .personas-panel {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 20px 22px 24px;
    overflow-y: auto;
    animation: page-entrance var(--motion-page-entrance, 200ms) ease-out;
  }

  .panel-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    margin-bottom: 18px;
  }

  .panel-head h2 {
    margin: 0;
    font-size: 18px;
  }

  .subtitle {
    margin: 4px 0 0;
    font-size: 12px;
    color: var(--fg-muted);
    max-width: 52ch;
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }

  .close-btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 14px;
  }

  .card {
    position: relative;
    border: 1px solid color-mix(in srgb, var(--persona-color, var(--border)) 40%, var(--border));
    border-radius: 14px;
    background: color-mix(in srgb, var(--persona-color, var(--bg-card)) 8%, var(--bg-card));
    min-height: 132px;
    overflow: hidden;
  }

  .card-main {
    width: 100%;
    height: 100%;
    min-height: 132px;
    border: none;
    background: transparent;
    color: var(--fg);
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 16px;
    text-align: left;
    transition: background 120ms ease-out;
  }

  .card-main:hover:not(:disabled) {
    background: color-mix(in srgb, var(--persona-color) 12%, transparent);
  }

  .card-main:disabled {
    cursor: default;
    opacity: 0.7;
  }

  .avatar {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    border-radius: 12px;
    font-size: 24px;
    background: color-mix(in srgb, var(--persona-color) 22%, var(--bg));
    border: 1px solid color-mix(in srgb, var(--persona-color) 45%, transparent);
  }

  .card-name {
    font-size: 15px;
    font-weight: 700;
  }

  .card-platforms {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 8px;
  }

  .chip {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.02em;
  }

  .chip.empty {
    color: var(--fg-subtle);
    text-transform: none;
    font-style: italic;
  }

  .switching {
    font-size: 11px;
    color: var(--persona-color);
    font-weight: 600;
  }

  .edit-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: 6px;
    background: color-mix(in srgb, var(--bg) 60%, transparent);
    color: var(--fg-muted);
    cursor: pointer;
    opacity: 0;
    transition: opacity 120ms, background 120ms, color 120ms;
  }

  .card:hover .edit-btn {
    opacity: 1;
  }

  .edit-btn:hover:not(:disabled) {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .edit-btn:disabled {
    cursor: default;
  }

  .new-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 132px;
    border: 1px dashed var(--border);
    border-radius: 14px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: border-color 120ms, color 120ms, background 120ms;
  }

  .new-card:hover {
    border-color: color-mix(in srgb, var(--fg) 30%, var(--border));
    color: var(--fg);
    background: var(--bg-card);
  }

  .empty-hint {
    margin: 18px 2px 0;
    font-size: 12px;
    color: var(--fg-subtle);
  }

  :global(html[data-motion="reduced"]) .personas-panel {
    animation: none;
  }
</style>
