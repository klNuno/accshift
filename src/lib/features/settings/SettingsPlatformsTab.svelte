<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getPlatformDefinition } from "$lib/platforms/registry";
  import { getBootPayload } from "$lib/app/bootPayload";
  import ConfirmDialog from "$lib/shared/components/ConfirmDialog.svelte";
  import DescriptorPreviewDialog from "$lib/platforms/DescriptorPreviewDialog.svelte";
  import {
    installDescriptorFile,
    openDescriptorsFolder,
    previewDescriptorFile,
    reloadUserPlatforms,
    removeDescriptor,
    selectDescriptorFile,
    type DescriptorPreview,
    type UserPlatformReport,
  } from "$lib/platforms/descriptors";
  import { ALL_PLATFORMS } from "./store";
  import type { MessageKey, TranslationParams } from "$lib/i18n";
  import type { AppSettings } from "./types";
  import type { RuntimeOs } from "$lib/shared/platform";

  let {
    settings = $bindable(),
    platformPaths = $bindable(),
    t,
    runtimeOs = "unknown",
    registerSearchInput = () => {},
  }: {
    settings: AppSettings;
    platformPaths: Record<string, string>;
    t: (key: MessageKey, params?: TranslationParams) => string;
    runtimeOs?: RuntimeOs;
    registerSearchInput?: (node: HTMLInputElement | null) => void;
  } = $props();

  function trackSearchInput(node: HTMLInputElement) {
    registerSearchInput(node);
    return {
      destroy() {
        registerSearchInput(null);
      },
    };
  }

  let platformSearch = $state("");

  // `ALL_PLATFORMS` is a plain array the descriptor calls edit in place, so the
  // lists below read this copy instead: adding or removing a descriptor swaps
  // it, and everything derived from it redraws.
  let knownPlatforms = $state([...ALL_PLATFORMS]);

  let visiblePlatformOptions = $derived.by(() =>
    knownPlatforms.filter((platform) => platform.implemented || settings.enabledPlatforms.includes(platform.id))
  );

  let filteredPlatformOptions = $derived.by(() => {
    const query = platformSearch.trim().toLowerCase();
    if (!query) return visiblePlatformOptions;
    return visiblePlatformOptions.filter((platform) =>
      platform.name.toLowerCase().includes(query) || platform.id.toLowerCase().includes(query),
    );
  });

  function isPlatformOsCompatible(platformId: string): boolean {
    const definition = getPlatformDefinition(platformId);
    if (!definition) return false;
    return definition.supportedOs.includes(runtimeOs);
  }

  function isPlatformSelectable(platformId: string): boolean {
    const definition = getPlatformDefinition(platformId);
    if (!definition) return false;
    return definition.implemented && isPlatformOsCompatible(platformId);
  }

  function platformAvailabilityLabel(platformId: string): string {
    const definition = getPlatformDefinition(platformId);
    if (!definition) return t("settings.platformNotImplemented");
    if (!definition.implemented) return t("settings.platformNotImplemented");
    if (!isPlatformOsCompatible(platformId)) return t("settings.platformUnsupportedOs");
    // Where a platform came from is worth saying: a descriptor the user wrote
    // is theirs to fix, and it disappears the day they delete the file.
    if (definition.userProvided) return t("settings.platformUserProvided");
    return "";
  }

  // What the backend last read in the descriptor folder. A file that was
  // refused says so here or nowhere: the engine names the offending field, and
  // dropping that on the floor is exactly the silent failure descriptors are
  // meant to replace.
  let userPlatforms = $state<UserPlatformReport | undefined>(getBootPayload()?.userPlatforms);

  let descriptorProblems = $derived([
    ...(userPlatforms?.rejected ?? []).map((entry) => ({
      name: entry.source,
      detail: entry.field ? `${entry.field}: ${entry.problem}` : entry.problem,
    })),
    ...(userPlatforms?.skipped ?? []).map((entry) => ({
      name: entry.id,
      detail: entry.reason,
    })),
  ]);

  let descriptorBusy = $state(false);
  let descriptorNotice = $state("");
  let descriptorError = $state("");
  let preview = $state<DescriptorPreview | null>(null);
  // The preview only carries the file's name, for display. Installing needs the
  // path the picker returned, so it is held here until the user decides.
  let previewPath = $state("");
  let pendingRemoval = $state<{ id: string; name: string } | null>(null);

  /** Takes a fresh report and republishes the lists that were built from it. */
  function applyReport(report: UserPlatformReport) {
    userPlatforms = report;
    knownPlatforms = [...ALL_PLATFORMS];
  }

  /**
   * Runs one folder operation with the buttons disabled, and puts whatever the
   * backend says on screen. A descriptor that will not load is the user's file
   * to fix, so the engine's own wording reaches them rather than a generic
   * failure.
   */
  async function runDescriptorTask(task: () => Promise<void>) {
    descriptorBusy = true;
    descriptorError = "";
    try {
      await task();
    } catch (error) {
      descriptorNotice = "";
      descriptorError = String(error);
    } finally {
      descriptorBusy = false;
    }
  }

  function reloadDescriptors() {
    void runDescriptorTask(async () => {
      applyReport(await reloadUserPlatforms());
      descriptorNotice = "";
    });
  }

  function pickDescriptor() {
    void runDescriptorTask(async () => {
      const path = await selectDescriptorFile();
      if (!path) return;
      descriptorNotice = "";
      preview = await previewDescriptorFile(path);
      previewPath = path;
    });
  }

  function confirmInstall() {
    const picked = preview;
    if (!picked) return;
    const path = previewPath;
    void runDescriptorTask(async () => {
      applyReport(await installDescriptorFile(path));
      descriptorNotice = t("descriptor.installed", { platform: picked.descriptor.name });
      preview = null;
    });
  }

  function confirmRemoval() {
    const target = pendingRemoval;
    pendingRemoval = null;
    if (!target) return;
    void runDescriptorTask(async () => {
      applyReport(await removeDescriptor(target.id));
      descriptorNotice = t("descriptor.removed", { platform: target.name });
      // The platform stops existing, so it cannot stay in the enabled set.
      settings.enabledPlatforms = settings.enabledPlatforms.filter((id) => id !== target.id);
    });
  }

  function openFolder() {
    void runDescriptorTask(() => openDescriptorsFolder());
  }

  function togglePlatform(id: string) {
    if (settings.enabledPlatforms.includes(id)) {
      const selectableEnabled = settings.enabledPlatforms.filter((platformId) => isPlatformSelectable(platformId));
      if (selectableEnabled.length <= 1 && selectableEnabled.includes(id)) return;
      settings.enabledPlatforms = settings.enabledPlatforms.filter((platformId) => platformId !== id);
    } else {
      if (!isPlatformSelectable(id)) return;
      settings.enabledPlatforms = [...settings.enabledPlatforms, id];
      if (!(id in platformPaths)) {
        platformPaths[id] = "";
        void invoke<string>("platform_get_path", { platformId: id })
          .then((path) => {
            if (settings.enabledPlatforms.includes(id)) {
              platformPaths[id] = path;
            }
          })
          .catch(() => {});
      }
    }
  }
</script>

<div class="settings-grid">
  <section class="card card-wide">
    <div class="card-title-row">
      <h3>{t("settings.platforms")}</h3>
      <label class="platform-search">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        <input
          type="search"
          placeholder={t("settings.platformSearchPlaceholder")}
          bind:value={platformSearch}
          use:trackSearchInput
        />
      </label>
    </div>
    <div class="platforms">
      {#each filteredPlatformOptions as platform (platform.id)}
        {@const isSelectable = isPlatformSelectable(platform.id)}
        {@const isEnabled = settings.enabledPlatforms.includes(platform.id)}
        {@const isLocked = !isSelectable && !isEnabled}
        {@const statusLabel = platformAvailabilityLabel(platform.id)}
        <button
          class="platform-chip"
          class:disabled={isLocked}
          role="switch"
          aria-checked={isEnabled}
          onclick={() => togglePlatform(platform.id)}
          style={`--chip-accent:${platform.accent};`}
          disabled={isLocked}
          title={statusLabel || platform.name}
        >
          <span class="platform-main">
            <span>{platform.name}</span>
            {#if statusLabel}
              <span class="platform-status">{statusLabel}</span>
            {/if}
          </span>
          <div class="toggle" class:active={isEnabled} aria-hidden="true">
            <div class="knob"></div>
          </div>
        </button>
      {:else}
        <p class="no-results">{t("settings.platformSearchNoResults")}</p>
      {/each}
    </div>
  </section>

  <section class="card">
    <h3>{t("settings.startupAndExtras")}</h3>
    <label class="field">
      <span class="field-label">{t("settings.defaultOnStartup")}</span>
      <select class="text-input select-input" bind:value={settings.defaultPlatformId}>
        {#each visiblePlatformOptions as platform}
          {@const disabled = !settings.enabledPlatforms.includes(platform.id) || !isPlatformSelectable(platform.id)}
          <option value={platform.id} {disabled}>
            {platform.name}{disabled ? ` ${t("settings.platformDisabledSuffix")}` : ""}
          </option>
        {/each}
      </select>
    </label>
    <button
      class="platform-chip"
      role="switch"
      aria-checked={settings.personasEnabled}
      onclick={() => (settings.personasEnabled = !settings.personasEnabled)}
      style="--chip-accent:#a855f7;"
      title={t("personas.title")}
    >
      <span class="platform-main">
        <span>{t("personas.title")}</span>
        <span class="platform-status">{t("settings.personasHint")}</span>
      </span>
      <div class="toggle" class:active={settings.personasEnabled} aria-hidden="true">
        <div class="knob"></div>
      </div>
    </button>
  </section>

  {#if userPlatforms?.dir}
    <section class="card card-wide">
      <h3>{t("settings.customPlatforms")}</h3>
      <div class="descriptor-head">
        <p class="descriptor-hint">{t("settings.customPlatformsHint")}</p>
        <p class="descriptor-dir">{userPlatforms.dir}</p>
      </div>
      <div class="descriptor-actions">
        <button class="descriptor-button primary" disabled={descriptorBusy} onclick={pickDescriptor}>
          {t("descriptor.addFromFile")}
        </button>
        <button class="descriptor-button" disabled={descriptorBusy} onclick={reloadDescriptors}>
          {t("descriptor.reload")}
        </button>
        <button class="descriptor-button" disabled={descriptorBusy} onclick={openFolder}>
          {t("descriptor.openFolder")}
        </button>
      </div>
      {#if descriptorError}
        <p class="descriptor-detail descriptor-failed">{descriptorError}</p>
      {:else if descriptorNotice}
        <p class="descriptor-detail">{descriptorNotice}</p>
      {/if}
      {#if userPlatforms.loaded.length}
        <ul class="descriptor-list">
          {#each userPlatforms.loaded as descriptor (descriptor.id)}
            <li class="descriptor-row">
              <span class="descriptor-name">{descriptor.name}</span>
              <code>{descriptor.id}</code>
              <button
                class="descriptor-button descriptor-remove"
                disabled={descriptorBusy}
                onclick={() => (pendingRemoval = { id: descriptor.id, name: descriptor.name })}
              >
                {t("descriptor.remove")}
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="descriptor-hint">{t("settings.customPlatformsNone")}</p>
      {/if}
      {#if descriptorProblems.length}
        <h4 class="descriptor-subtitle">{t("settings.customPlatformsProblems")}</h4>
        <ul class="descriptor-list">
          {#each descriptorProblems as problem (problem.name + problem.detail)}
            <li><span class="descriptor-name">{problem.name}</span> <span class="descriptor-detail">{problem.detail}</span></li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>

{#if preview}
  <DescriptorPreviewDialog
    preview={preview}
    busy={descriptorBusy}
    {t}
    onCancel={() => (preview = null)}
    onConfirm={confirmInstall}
  />
{/if}

{#if pendingRemoval}
  <ConfirmDialog
    title={t("descriptor.removeConfirmTitle", { platform: pendingRemoval.name })}
    message={t("descriptor.removeConfirmMessage")}
    confirmLabel={t("descriptor.remove")}
    cancelLabel={t("common.cancel")}
    onConfirm={confirmRemoval}
    onCancel={() => (pendingRemoval = null)}
  />
{/if}

<style>
  .platforms {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  /* The row carries the header divider so it spans past the search pill. */
  .card-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-bottom: 8px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 65%, transparent);
  }

  .card-title-row h3 {
    margin: 0;
    padding-bottom: 0;
    border-bottom: none;
  }

  .platform-search {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    width: 200px;
    padding: 6px 11px;
    border: 1px solid transparent;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-card) 88%, #fff 12%);
    color: var(--fg-subtle);
    cursor: text;
    transition: border-color 120ms ease-out, background 120ms ease-out, color 120ms ease-out;
  }

  .platform-search:hover {
    background: color-mix(in srgb, var(--bg-card) 84%, #fff 16%);
  }

  .platform-search:focus-within {
    border-color: color-mix(in srgb, var(--fg-muted) 45%, var(--border));
    color: var(--fg-muted);
  }

  .platform-search svg {
    flex: 0 0 auto;
  }

  .platform-search input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: transparent;
    color: var(--fg);
    font-size: 12px;
    padding: 0;
  }

  .platform-search input::placeholder {
    color: var(--fg-subtle);
  }

  .platform-search input::-webkit-search-cancel-button {
    -webkit-appearance: none;
  }

  .descriptor-head {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .descriptor-hint {
    margin: 0;
    font-size: 12px;
    color: var(--fg-subtle);
  }

  /* Wraps rather than clipping: the point of showing the folder is that the
     user can find it, and a truncated path does not help with that. */
  .descriptor-dir {
    margin: 0;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 11px;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  .descriptor-subtitle {
    margin: 4px 0 0;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .descriptor-list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
  }

  .descriptor-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  /* The remove button sits at the far edge so it is never next to the name it
     would delete, and the row reads name first, action last. */
  .descriptor-remove {
    margin-left: auto;
  }

  .descriptor-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .descriptor-button {
    padding: 5px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 11px;
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out, color 120ms ease-out;
  }

  .descriptor-button:hover:not(:disabled) {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .descriptor-button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Adding a platform is the action this card exists for, so it carries the
     app's primary button rather than sitting level with reload and open. */
  .descriptor-button.primary {
    border-color: transparent;
    background: var(--fg);
    color: var(--bg-solid);
    font-weight: 600;
  }

  .descriptor-button.primary:hover:not(:disabled) {
    background: var(--fg);
    color: var(--bg-solid);
    filter: brightness(0.9);
  }

  .descriptor-name {
    color: var(--fg);
  }

  .descriptor-detail,
  .descriptor-list code {
    color: var(--fg-subtle);
    font-size: 11px;
  }

  .descriptor-failed {
    color: var(--danger);
  }

  .no-results {
    margin: 0;
    font-size: 12px;
    color: var(--fg-subtle);
    padding: 6px 2px;
  }

  .platform-chip {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg-card) 88%, #fff 12%);
    color: var(--fg);
    padding: 10px 12px;
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out;
  }

  .platform-main {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
  }

  .platform-status {
    font-size: 10px;
    color: var(--fg-subtle);
  }

  .platform-chip:hover {
    border-color: color-mix(in srgb, var(--chip-accent) 55%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 84%, #fff 16%);
  }

  .platform-chip.disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .platform-chip.disabled:hover {
    border-color: var(--border);
    background: color-mix(in srgb, var(--bg-card) 88%, #fff 12%);
  }

  .toggle {
    width: 36px;
    height: 20px;
    border-radius: 999px;
    background: var(--bg-elevated);
    padding: 2px;
    transition: background 120ms ease-out;
  }

  .toggle.active {
    background: var(--chip-accent);
  }

  .knob {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    transition: transform 120ms ease-out;
  }

  .toggle.active .knob {
    transform: translateX(16px);
  }
</style>
