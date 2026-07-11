<script lang="ts">
  import { onMount } from "svelte";
  import type { Persona } from "./types";
  import PersonaCover from "./PersonaCover.svelte";
  import type { CoverTile } from "./PersonaCover.svelte";
  import PersonaWizard from "./PersonaWizard.svelte";
  import { buildContextMenuItems } from "$lib/shared/contextMenu/types";
  import { ACCOUNT_CARD_COLOR_PRESETS } from "$lib/shared/accountCardColors";
  import { COLOR_LABEL_KEYS } from "$lib/shared/contextMenu/accountAppearanceActions";
  import { getPlatform } from "$lib/shared/platform";
  import type { PlatformAccount } from "$lib/shared/platform";
  import type { ContextMenuItem } from "$lib/shared/types";
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
    requestConfirm,
    showToast,
    openContextMenu,
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
    requestConfirm: (config: {
      title: string;
      message: string;
      confirmLabel?: string;
      confirmColor?: string;
    }) => Promise<boolean>;
    showToast: (message: string, options?: { type?: "success" | "error" }) => void;
    /** Opens the app-level context menu (same positioning as account cards). */
    openContextMenu: (event: MouseEvent, items: ContextMenuItem[]) => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();

  // null = grid, "new" = create wizard, otherwise the persona being edited
  let editing = $state<Persona | "new" | null>(null);
  let accountsByPlatform = $state<Record<string, PlatformAccount[]>>({});
  let accountsLoading = $state(true);
  // "platformId:accountId" -> resolved avatar url (null = known to have none)
  let avatarByKey = $state<Record<string, string | null>>({});

  function accent(platformId: string): string {
    return platforms.find((p) => p.id === platformId)?.accent ?? "var(--fg-subtle)";
  }

  function avatarFor(platformId: string, accountId: string): string | null {
    return avatarByKey[`${platformId}:${accountId}`] ?? null;
  }

  async function resolveAvatar(platformId: string, accountId: string) {
    const key = `${platformId}:${accountId}`;
    if (key in avatarByKey) return;
    avatarByKey[key] = null;
    const adapter = getPlatform(platformId);
    if (!adapter) return;
    const cached = adapter.getCachedProfile?.(accountId);
    if (cached?.url) {
      avatarByKey[key] = cached.url;
      if (!cached.expired) return;
    }
    if (!adapter.getProfileInfo) return;
    try {
      const profile = await adapter.getProfileInfo(accountId);
      if (profile?.avatarUrl) avatarByKey[key] = profile.avatarUrl;
    } catch {
      // Avatars are decoration; the platform-icon fallback covers this.
    }
  }

  onMount(async () => {
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
    accountsLoading = false;
    // Resolve avatars for every known account: the mosaics need the assigned
    // ones and the wizard's pickers need the rest. Cached profiles answer
    // instantly; the rest fetch best-effort in the background.
    for (const [platformId, accounts] of Object.entries(loaded)) {
      for (const account of accounts) {
        void resolveAvatar(platformId, account.id);
      }
    }
  });

  function coverTilesFor(persona: Persona): CoverTile[] {
    return persona.assignments.map((a) => ({
      key: `${a.platformId}:${a.accountId}`,
      avatarUrl: avatarFor(a.platformId, a.accountId),
      accent: accent(a.platformId),
      platformId: a.platformId,
    }));
  }

  function handleSave(input: { name: string; image: string | null; assignments: Persona["assignments"] }) {
    if (editing && editing !== "new") {
      onUpdate(editing.id, input);
    } else {
      onCreate({ ...input, color: "" });
    }
    editing = null;
  }

  function handlePersonaContextMenu(event: MouseEvent, persona: Persona) {
    event.preventDefault();
    if (switchingPersonaId) return;
    openContextMenu(event, buildPersonaMenuItems(persona));
  }

  async function confirmDelete(persona: Persona) {
    const confirmed = await requestConfirm({
      title: t("personas.deleteTitle"),
      message: t("personas.deleteMessage", { name: persona.name }),
      confirmLabel: t("personas.delete"),
      confirmColor: "var(--danger, #ef4444)",
    });
    if (confirmed) onDelete(persona.id);
  }

  function buildPersonaMenuItems(persona: Persona): ContextMenuItem[] {
    return buildContextMenuItems(
      [
        {
          id: "persona.edit",
          group: "persona.primary",
          label: t("personas.edit"),
          action: () => {
            editing = persona;
          },
        },
        {
          id: "persona.color",
          group: "persona.appearance",
          kind: "swatches",
          label: t("context.menu.cardColor"),
          swatches: ACCOUNT_CARD_COLOR_PRESETS.map((preset) => ({
            id: preset.id,
            label: t(COLOR_LABEL_KEYS[preset.id]),
            color: preset.color,
            active: persona.color === preset.color,
            action: () => onUpdate(persona.id, { color: preset.color }),
          })),
        },
        {
          id: "persona.delete",
          group: "persona.danger",
          label: t("personas.delete"),
          action: () => void confirmDelete(persona),
        },
      ],
      ["persona.primary", "persona.appearance", "persona.danger"],
    );
  }
</script>

<div class="personas-panel">
  {#if editing}
    <PersonaWizard
      persona={editing === "new" ? null : editing}
      {platforms}
      {accountsByPlatform}
      loading={accountsLoading}
      {avatarFor}
      onSave={handleSave}
      onCancel={() => (editing = null)}
      {showToast}
      {t}
    />
  {:else}
    <div class="grid">
      {#each personas as persona (persona.id)}
        {@const busy = switchingPersonaId === persona.id}
        <button
          class="card"
          class:tinted={!!persona.color}
          class:busy
          style={persona.color ? `--persona-color:${persona.color};` : ""}
          disabled={switchingPersonaId !== null}
          onclick={() => onSwitch(persona)}
          oncontextmenu={(e) => handlePersonaContextMenu(e, persona)}
          title={t("personas.activate")}
        >
          <PersonaCover image={persona.image} tiles={coverTilesFor(persona)} />
          <span class="card-name">{persona.name}</span>
          {#if busy}
            <span class="switching">{t("personas.switching")}</span>
          {/if}
        </button>
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
  {/if}
</div>

<style>
  .personas-panel {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 18px 22px 24px;
    overflow-y: auto;
    animation: page-entrance var(--motion-page-entrance, 200ms) ease-out;
  }

  :global(html[data-motion="reduced"]) .personas-panel {
    animation: none;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 14px;
  }

  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--bg-card);
    color: var(--fg);
    padding: 10px;
    cursor: pointer;
    text-align: left;
    transition: border-color 120ms ease-out, background 120ms ease-out, transform 120ms ease-out;
  }

  .card:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--fg) 30%, var(--border));
    transform: translateY(-1px);
  }

  .card.tinted {
    border-color: color-mix(in srgb, var(--persona-color) 45%, var(--border));
    background: color-mix(in srgb, var(--persona-color) 8%, var(--bg-card));
  }

  .card.tinted:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--persona-color) 70%, var(--border));
  }

  .card:disabled {
    cursor: default;
  }

  .card:disabled:not(.busy) {
    opacity: 0.6;
  }

  .card-name {
    font-size: 13px;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0 2px 2px;
  }

  .switching {
    position: absolute;
    inset: 10px 10px auto 10px;
    aspect-ratio: 1 / 1;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg) 65%, transparent);
    backdrop-filter: blur(2px);
    font-size: 11px;
    font-weight: 700;
    color: var(--fg);
  }

  .new-card {
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 150px;
    border-style: dashed;
    background: transparent;
    color: var(--fg-muted);
    font-size: 12px;
    font-weight: 600;
  }

  .new-card:hover {
    color: var(--fg);
    background: var(--bg-card);
    transform: none;
  }

  .empty-hint {
    margin: 16px 2px 0;
    font-size: 12px;
    color: var(--fg-subtle);
  }
</style>
