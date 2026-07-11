<script lang="ts">
  import { onMount } from "svelte";
  import type { Persona } from "./types";
  import PersonaCover from "./PersonaCover.svelte";
  import type { CoverTile } from "./PersonaCover.svelte";
  import { PLATFORM_ICON_PATHS } from "$lib/shared/platformIcons";
  import type { PlatformAccount } from "$lib/shared/platform";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    persona = null,
    platforms,
    accountsByPlatform,
    loading,
    avatarFor,
    onSave,
    onCancel,
    showToast,
    t,
  }: {
    persona?: Persona | null;
    platforms: { id: string; name: string; accent: string }[];
    accountsByPlatform: Record<string, PlatformAccount[]>;
    loading: boolean;
    avatarFor: (platformId: string, accountId: string) => string | null;
    onSave: (input: { name: string; image: string | null; assignments: Persona["assignments"] }) => void;
    onCancel: () => void;
    showToast: (message: string, options?: { type?: "success" | "error" }) => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();

  // The wizard is mounted fresh per open (PersonasPanel toggles it under an
  // {#if}), so seeding local state from the props' initial values is the
  // intended behavior.
  // svelte-ignore state_referenced_locally
  let name = $state(persona?.name ?? "");
  // svelte-ignore state_referenced_locally
  let image = $state<string | null>(persona?.image ?? null);
  // platformId -> selected accountId ("" = not part of this persona)
  // svelte-ignore state_referenced_locally
  let selection = $state<Record<string, string>>(
    Object.fromEntries(
      platforms.map((p) => [
        p.id,
        persona?.assignments.find((a) => a.platformId === p.id)?.accountId ?? "",
      ]),
    ),
  );
  // null = platform grid, otherwise the platform whose account is being picked
  let picking = $state<string | null>(null);
  let confirmingCancel = $state(false);
  let cancelResetTimer: ReturnType<typeof setTimeout> | undefined;
  let nameInputRef = $state<HTMLInputElement | null>(null);
  let fileInputRef = $state<HTMLInputElement | null>(null);

  // svelte-ignore state_referenced_locally
  const baseline = JSON.stringify([
    persona?.name ?? "",
    persona?.image ?? null,
    persona?.assignments ?? [],
  ]);

  onMount(() => {
    if (!persona) nameInputRef?.focus();
  });

  // A stale accountId (account removed since the persona was saved) must never
  // survive a save; while accounts are still loading, trust the stored value.
  function isValidAssignment(platformId: string): boolean {
    const id = selection[platformId];
    if (!id) return false;
    if (loading) return true;
    return (accountsByPlatform[platformId] ?? []).some((a) => a.id === id);
  }

  function assignedAccount(platformId: string): PlatformAccount | null {
    const id = selection[platformId];
    if (!id) return null;
    return (accountsByPlatform[platformId] ?? []).find((a) => a.id === id) ?? null;
  }

  let assignments = $derived(
    platforms
      .filter((p) => isValidAssignment(p.id))
      .map((p) => ({ platformId: p.id, accountId: selection[p.id] })),
  );
  let dirty = $derived(JSON.stringify([name, image, assignments]) !== baseline);
  let canSave = $derived(name.trim().length > 0 && assignments.length > 0 && !loading);

  let coverTiles = $derived<CoverTile[]>(
    assignments.map((a) => ({
      key: `${a.platformId}:${a.accountId}`,
      avatarUrl: avatarFor(a.platformId, a.accountId),
      accent: platforms.find((p) => p.id === a.platformId)?.accent ?? "var(--fg-subtle)",
      platformId: a.platformId,
    })),
  );

  function save() {
    if (!canSave) return;
    onSave({ name: name.trim(), image, assignments });
  }

  function requestCancel() {
    if (!dirty || confirmingCancel) {
      onCancel();
      return;
    }
    confirmingCancel = true;
    clearTimeout(cancelResetTimer);
    cancelResetTimer = setTimeout(() => (confirmingCancel = false), 3000);
  }

  function pickAccount(platformId: string, accountId: string) {
    selection[platformId] = accountId;
    picking = null;
  }

  function unassign(platformId: string) {
    selection[platformId] = "";
    picking = null;
  }

  async function decodeImage(file: File): Promise<ImageBitmap | HTMLImageElement> {
    // createImageBitmap decodes straight from the file, no intermediate URL —
    // the app CSP does not allow blob: sources, so an <img src=blobUrl>
    // round-trip would be refused before it ever decoded.
    if (typeof createImageBitmap === "function") {
      return createImageBitmap(file);
    }
    const dataUrl = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(new Error("read failed"));
      reader.readAsDataURL(file);
    });
    const img = new Image();
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = () => reject(new Error("decode failed"));
      img.src = dataUrl;
    });
    return img;
  }

  async function handleImageFile(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    try {
      const source = await decodeImage(file);
      // Downscale + center-crop to a small square so the stored data URL stays
      // tiny; the cover never renders bigger than ~200px.
      const size = 256;
      const canvas = document.createElement("canvas");
      canvas.width = size;
      canvas.height = size;
      const ctx = canvas.getContext("2d");
      if (!ctx) throw new Error("no 2d context");
      const scale = Math.max(size / source.width, size / source.height);
      const w = source.width * scale;
      const h = source.height * scale;
      ctx.drawImage(source, (size - w) / 2, (size - h) / 2, w, h);
      if ("close" in source) source.close();
      let data = canvas.toDataURL("image/webp", 0.85);
      if (!data.startsWith("data:image/webp")) data = canvas.toDataURL("image/jpeg", 0.85);
      image = data;
    } catch {
      showToast(t("personas.imageError"), { type: "error" });
    }
  }

  let pickingPlatform = $derived(platforms.find((p) => p.id === picking) ?? null);
</script>

<div class="wizard">
  <header class="head">
    <button class="icon-btn" onclick={picking ? () => (picking = null) : requestCancel} aria-label={t("common.back")} class:armed={!picking && confirmingCancel}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M19 12H5" />
        <path d="m12 19-7-7 7-7" />
      </svg>
    </button>

    <div class="cover-slot">
      <PersonaCover {image} tiles={coverTiles} />
    </div>

    <div class="head-fields">
      <input
        bind:this={nameInputRef}
        class="name-input"
        placeholder={t("personas.namePlaceholder")}
        maxlength={40}
        bind:value={name}
      />
      <div class="image-actions">
        <button class="mini-btn" onclick={() => fileInputRef?.click()}>
          {t("personas.customImage")}
        </button>
        {#if image}
          <button class="mini-btn" onclick={() => (image = null)}>
            {t("personas.removeImage")}
          </button>
        {/if}
      </div>
      <input
        bind:this={fileInputRef}
        type="file"
        accept="image/*"
        class="file-input"
        onchange={handleImageFile}
      />
    </div>

    <div class="head-spacer"></div>

    <button class="finish-btn" disabled={!canSave} onclick={save}>
      {t("personas.finish")}
    </button>
  </header>

  {#if picking && pickingPlatform}
    <!-- Account picker: the platform's accounts as neutral cards, no colors,
         same spirit as Steam's bulk edit selection. -->
    <div class="picker">
      <div class="picker-head" style={`--p-accent:${pickingPlatform.accent};`}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
          <path d={PLATFORM_ICON_PATHS[pickingPlatform.id] ?? ""} />
        </svg>
        <span>{t("personas.chooseAccount", { platform: pickingPlatform.name })}</span>
        <span class="picker-spacer"></span>
        {#if selection[picking]}
          <button
            class="icon-btn unassign-btn"
            onclick={() => unassign(picking!)}
            title={t("personas.removeFromPersona")}
            aria-label={t("personas.removeFromPersona")}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        {/if}
      </div>

      {#if loading}
        <p class="hint">{t("personas.loadingAccounts")}</p>
      {:else}
        {@const accounts = accountsByPlatform[picking] ?? []}
        {#if accounts.length === 0}
          <p class="hint">{t("personas.noAccountsOnPlatform")}</p>
        {:else}
          <div class="account-grid">
            {#each accounts as account (account.id)}
              {@const avatarUrl = avatarFor(picking, account.id)}
              <button
                class="account-card"
                class:selected={selection[picking] === account.id}
                onclick={() => pickAccount(picking!, account.id)}
              >
                <span class="account-avatar" style={`--p-accent:${pickingPlatform.accent};`}>
                  {#if avatarUrl}
                    <img src={avatarUrl} alt="" draggable="false" loading="lazy" />
                  {:else}
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                      <path d={PLATFORM_ICON_PATHS[picking] ?? ""} />
                    </svg>
                  {/if}
                </span>
                <span class="account-names">
                  <span class="account-name">{account.displayName || account.username || account.id}</span>
                  {#if account.username && account.username !== account.displayName}
                    <span class="account-sub">{account.username}</span>
                  {/if}
                </span>
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  {:else}
    <p class="hint intro">{t("personas.choosePlatforms")}</p>
    <div class="platform-grid">
      {#each platforms as platform (platform.id)}
        {@const account = assignedAccount(platform.id)}
        {@const assigned = isValidAssignment(platform.id)}
        <button
          class="platform-card"
          class:assigned
          style={`--p-accent:${platform.accent};`}
          onclick={() => (picking = platform.id)}
        >
          <svg class="platform-icon" width="26" height="26" viewBox="0 0 24 24" fill="currentColor">
            <path d={PLATFORM_ICON_PATHS[platform.id] ?? ""} />
          </svg>
          <span class="platform-name">{platform.name}</span>
          {#if assigned && account}
            {@const avatarUrl = avatarFor(platform.id, account.id)}
            <span class="assigned-account">
              {#if avatarUrl}
                <img src={avatarUrl} alt="" draggable="false" loading="lazy" />
              {/if}
              <span>{account.displayName || account.username || account.id}</span>
            </span>
          {:else if assigned}
            <span class="assigned-account"><span>{selection[platform.id]}</span></span>
          {:else}
            <span class="assigned-account empty">&mdash;</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .wizard {
    display: flex;
    flex-direction: column;
    gap: 14px;
    animation: page-entrance var(--motion-page-entrance, 200ms) ease-out;
  }

  :global(html[data-motion="reduced"]) .wizard {
    animation: none;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .icon-btn {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    transition: background 120ms ease-out, color 120ms ease-out;
  }

  .icon-btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .icon-btn.armed {
    background: var(--danger, #ef4444);
    color: #fff;
  }

  .cover-slot {
    width: 72px;
    flex-shrink: 0;
  }

  .head-fields {
    display: flex;
    flex-direction: column;
    gap: 7px;
    min-width: 0;
  }

  .name-input {
    width: min(320px, 100%);
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

  .image-actions {
    display: flex;
    gap: 8px;
  }

  .file-input {
    display: none;
  }

  .mini-btn {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 11px;
    font-weight: 600;
    padding: 4px 9px;
    cursor: pointer;
    transition: border-color 120ms ease-out, color 120ms ease-out, background 120ms ease-out;
  }

  .mini-btn:hover {
    color: var(--fg);
    border-color: color-mix(in srgb, var(--fg) 30%, var(--border));
    background: var(--bg-card);
  }

  .head-spacer {
    flex: 1;
  }

  .finish-btn {
    border: none;
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent, #3b82f6) 88%, #000 12%);
    color: #fff;
    padding: 9px 18px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    transition: opacity 120ms ease-out;
    flex-shrink: 0;
  }

  .finish-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--fg-subtle);
  }

  .hint.intro {
    margin-left: 2px;
  }

  .platform-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 10px;
  }

  .platform-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--bg-card);
    color: var(--fg-muted);
    padding: 14px;
    cursor: pointer;
    text-align: left;
    transition: border-color 120ms ease-out, background 120ms ease-out, color 120ms ease-out;
  }

  .platform-card:hover {
    border-color: color-mix(in srgb, var(--p-accent) 55%, var(--border));
    color: var(--fg);
  }

  .platform-card.assigned {
    border-color: color-mix(in srgb, var(--p-accent) 55%, var(--border));
    background: color-mix(in srgb, var(--p-accent) 8%, var(--bg-card));
    color: var(--fg);
  }

  .platform-card.assigned .platform-icon {
    color: var(--p-accent);
  }

  .platform-name {
    font-size: 12px;
    font-weight: 700;
  }

  .assigned-account {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--fg-muted);
    min-width: 0;
    max-width: 100%;
  }

  .assigned-account span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .assigned-account img {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    object-fit: cover;
    flex-shrink: 0;
  }

  .assigned-account.empty {
    color: var(--fg-subtle);
  }

  .picker {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .picker-head {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--fg);
  }

  .picker-head > svg {
    color: var(--p-accent);
  }

  .picker-spacer {
    flex: 1;
  }

  .unassign-btn {
    width: 26px;
    height: 26px;
  }

  .account-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 10px;
  }

  /* Deliberately neutral: no custom card colors here, selection is the only
     highlight (same principle as Steam's bulk edit grid). */
  .account-card {
    display: flex;
    align-items: center;
    gap: 10px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--bg-card);
    color: var(--fg);
    padding: 10px 12px;
    cursor: pointer;
    text-align: left;
    min-width: 0;
    transition: border-color 120ms ease-out, background 120ms ease-out;
  }

  .account-card:hover {
    border-color: color-mix(in srgb, var(--fg) 30%, var(--border));
  }

  .account-card.selected {
    border-color: var(--accent, #3b82f6);
    box-shadow: inset 0 0 0 1px var(--accent, #3b82f6);
  }

  .account-avatar {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    flex-shrink: 0;
    border-radius: 8px;
    overflow: hidden;
    background: color-mix(in srgb, var(--p-accent, var(--fg-subtle)) 16%, var(--bg-muted));
    color: color-mix(in srgb, var(--p-accent, var(--fg-subtle)) 75%, var(--fg-muted));
  }

  .account-avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .account-names {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .account-name {
    font-size: 13px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .account-sub {
    font-size: 11px;
    color: var(--fg-subtle);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

</style>
