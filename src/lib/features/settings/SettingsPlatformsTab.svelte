<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getPlatformDefinition } from "$lib/platforms/registry";
  import { ALL_PLATFORMS } from "./store";
  import type { MessageKey, TranslationParams } from "$lib/i18n";
  import type { AppSettings } from "./types";
  import type { RuntimeOs } from "$lib/shared/platform";

  let {
    settings = $bindable(),
    platformPaths = $bindable(),
    t,
    runtimeOs = "unknown",
  }: {
    settings: AppSettings;
    platformPaths: Record<string, string>;
    t: (key: MessageKey, params?: TranslationParams) => string;
    runtimeOs?: RuntimeOs;
  } = $props();

  let platformSearch = $state("");

  let visiblePlatformOptions = $derived.by(() =>
    ALL_PLATFORMS.filter((platform) => platform.implemented || settings.enabledPlatforms.includes(platform.id))
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
    return "";
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
</div>

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
