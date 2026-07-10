<script lang="ts">
  import { onMount } from "svelte";
  import type { Persona } from "./types";
  import { PERSONA_COLORS } from "./store";
  import type { PlatformAccount } from "$lib/shared/platform";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    persona = null,
    platforms,
    loadAccounts,
    onSave,
    onCancel,
    onDelete = null,
    t,
  }: {
    persona?: Persona | null;
    platforms: { id: string; name: string; accent: string }[];
    loadAccounts: (platformId: string) => Promise<PlatformAccount[]>;
    onSave: (input: Omit<Persona, "id">) => void;
    onCancel: () => void;
    onDelete?: (() => void) | null;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();

  // Seeded from the persona prop in onMount (the editor is remounted per open).
  let name = $state("");
  let emoji = $state("🎭");
  let color = $state<string>(PERSONA_COLORS[0]);
  // platformId -> selected accountId ("" means not part of this persona)
  let selection = $state<Record<string, string>>({});
  let accountsByPlatform = $state<Record<string, PlatformAccount[]>>({});
  let loading = $state(true);
  let nameInputRef = $state<HTMLInputElement | null>(null);
  let baseline = $state("");
  let confirmingClose = $state(false);
  let confirmingDelete = $state(false);
  let closeResetTimer: ReturnType<typeof setTimeout> | undefined;
  let deleteResetTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(async () => {
    nameInputRef?.focus();
    if (persona) {
      name = persona.name;
      emoji = persona.emoji;
      color = persona.color;
    }
    const initial: Record<string, string> = {};
    for (const p of platforms) {
      initial[p.id] = persona?.assignments.find((a) => a.platformId === p.id)?.accountId ?? "";
    }
    selection = initial;
    baseline = snapshot();

    const loaded: Record<string, PlatformAccount[]> = {};
    await Promise.all(
      platforms.map(async (p) => {
        try {
          loaded[p.id] = await loadAccounts(p.id);
        } catch {
          loaded[p.id] = [];
        }
      }),
    );
    accountsByPlatform = loaded;
    loading = false;
  });

  function snapshot(): string {
    return JSON.stringify([name, emoji, color, selection]);
  }

  let dirty = $derived(baseline !== "" && snapshot() !== baseline);

  // A stale accountId (account removed since the persona was saved) renders
  // the select as empty; treat it as unassigned so it never survives a save.
  function isValidAssignment(platformId: string): boolean {
    const id = selection[platformId];
    if (!id) return false;
    if (loading) return true;
    return (accountsByPlatform[platformId] ?? []).some((a) => a.id === id);
  }

  let assignmentCount = $derived.by(() => platforms.filter((p) => isValidAssignment(p.id)).length);
  let canSave = $derived(name.trim().length > 0 && assignmentCount > 0);

  function save() {
    if (!canSave || loading) return;
    const assignments = platforms
      .filter((p) => isValidAssignment(p.id))
      .map((p) => ({ platformId: p.id, accountId: selection[p.id] }));
    onSave({ name: name.trim(), emoji: emoji || "🎭", color, assignments });
  }

  function requestClose() {
    if (!dirty || confirmingClose) {
      onCancel();
      return;
    }
    confirmingClose = true;
    clearTimeout(closeResetTimer);
    closeResetTimer = setTimeout(() => (confirmingClose = false), 3000);
  }

  function requestDelete() {
    if (!onDelete) return;
    if (confirmingDelete) {
      onDelete();
      return;
    }
    confirmingDelete = true;
    clearTimeout(deleteResetTimer);
    deleteResetTimer = setTimeout(() => (confirmingDelete = false), 3000);
  }

  function handleWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") requestClose();
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="editor-backdrop" onclick={requestClose} role="presentation">
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="editor" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" tabindex="-1">
    <div class="editor-head">
      <span class="editor-avatar" style={`--persona-color:${color};`}>{emoji || "🎭"}</span>
      <input
        bind:this={nameInputRef}
        class="name-input"
        placeholder={t("personas.namePlaceholder")}
        maxlength={40}
        bind:value={name}
      />
    </div>

    <div class="row">
      <label class="mini">
        <span class="mini-label">{t("personas.emoji")}</span>
        <input class="emoji-input" maxlength={4} bind:value={emoji} />
      </label>
      <div class="mini colors">
        <span class="mini-label">{t("personas.color")}</span>
        <div class="swatches">
          {#each PERSONA_COLORS as c}
            <button
              type="button"
              class="swatch"
              class:selected={color === c}
              style={`background:${c};`}
              aria-label={c}
              onclick={() => (color = c)}
            ></button>
          {/each}
        </div>
      </div>
    </div>

    <div class="section-label">{t("personas.accounts")}</div>
    {#if loading}
      <p class="hint">{t("personas.loadingAccounts")}</p>
    {:else}
      <div class="platform-list">
        {#each platforms as p}
          {@const accounts = accountsByPlatform[p.id] ?? []}
          <label class="platform-row">
            <span class="platform-name" style={`color:${p.accent};`}>{p.name}</span>
            {#if accounts.length > 0}
              <select class="account-select" bind:value={selection[p.id]}>
                <option value="">{t("personas.notIncluded")}</option>
                {#each accounts as acc}
                  <option value={acc.id}>{acc.displayName || acc.username || acc.id}</option>
                {/each}
              </select>
            {:else}
              <span class="no-accounts">{t("personas.noAccountsOnPlatform")}</span>
            {/if}
          </label>
        {/each}
      </div>
    {/if}

    <div class="editor-actions">
      {#if onDelete}
        <button type="button" class="btn danger" class:armed={confirmingDelete} onclick={requestDelete}>
          {confirmingDelete ? t("personas.deleteConfirm") : t("personas.delete")}
        </button>
      {/if}
      <div class="spacer"></div>
      <button type="button" class="btn" class:armed={confirmingClose} onclick={requestClose}>
        {confirmingClose ? t("personas.discardConfirm") : t("common.cancel")}
      </button>
      <button type="button" class="btn primary" disabled={!canSave} onclick={save}>
        {t("personas.save")}
      </button>
    </div>
  </div>
</div>

<style>
  .editor-backdrop {
    position: absolute;
    inset: 0;
    z-index: 620;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.4);
    padding: 20px;
  }

  .editor {
    width: min(440px, 92vw);
    max-height: 88vh;
    overflow-y: auto;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
  }

  .editor-head {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .editor-avatar {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 46px;
    height: 46px;
    flex-shrink: 0;
    border-radius: 12px;
    font-size: 24px;
    background: color-mix(in srgb, var(--persona-color) 22%, var(--bg));
    border: 1px solid color-mix(in srgb, var(--persona-color) 45%, transparent);
  }

  .name-input {
    flex: 1;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--fg);
    font-size: 15px;
    font-weight: 600;
    padding: 9px 11px;
    outline: none;
  }

  .name-input:focus {
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
  }

  .row {
    display: flex;
    gap: 16px;
    align-items: flex-start;
  }

  .mini-label {
    display: block;
    font-size: 11px;
    color: var(--fg-subtle);
    margin-bottom: 5px;
  }

  .emoji-input {
    width: 60px;
    text-align: center;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--fg);
    font-size: 18px;
    padding: 6px;
    outline: none;
  }

  .colors {
    flex: 1;
  }

  .swatches {
    display: flex;
    gap: 7px;
    flex-wrap: wrap;
  }

  .swatch {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 2px solid transparent;
    cursor: pointer;
    padding: 0;
  }

  .swatch.selected {
    border-color: var(--fg);
    box-shadow: 0 0 0 2px var(--bg-card);
  }

  .section-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--fg-muted);
    border-top: 1px solid var(--border);
    padding-top: 12px;
  }

  .platform-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .platform-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .platform-name {
    font-size: 13px;
    font-weight: 600;
  }

  .account-select {
    flex: 1;
    max-width: 62%;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--fg);
    font-size: 12px;
    padding: 7px 9px;
    outline: none;
  }

  .no-accounts {
    font-size: 11px;
    color: var(--fg-subtle);
    font-style: italic;
  }

  .hint {
    font-size: 12px;
    color: var(--fg-subtle);
    margin: 0;
  }

  .editor-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    border-top: 1px solid var(--border);
    padding-top: 14px;
  }

  .spacer {
    flex: 1;
  }

  .btn {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bg-card) 88%, #fff 12%);
    color: var(--fg);
    padding: 8px 14px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out, opacity 120ms;
  }

  .btn:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn.primary {
    background: color-mix(in srgb, var(--accent, #3b82f6) 88%, #000 12%);
    border-color: transparent;
    color: #fff;
  }

  .btn.danger {
    color: var(--danger, #ef4444);
  }

  .btn.armed {
    background: var(--danger, #ef4444);
    border-color: transparent;
    color: #fff;
  }
</style>
