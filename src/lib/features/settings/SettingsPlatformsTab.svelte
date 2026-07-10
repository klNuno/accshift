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

  let visiblePlatformOptions = $derived.by(() =>
    ALL_PLATFORMS.filter((platform) => platform.implemented || settings.enabledPlatforms.includes(platform.id))
  );

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
    <h3>{t("settings.platforms")}</h3>
    <div class="platforms">
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
      {#each visiblePlatformOptions as platform}
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
      {/each}
    </div>
  </section>

  <section class="card">
    <h3>{t("settings.defaultOnStartup")}</h3>
    <label class="field">
      <select class="text-input select-input" bind:value={settings.defaultPlatformId}>
        {#each visiblePlatformOptions as platform}
          {@const disabled = !settings.enabledPlatforms.includes(platform.id) || !isPlatformSelectable(platform.id)}
          <option value={platform.id} {disabled}>
            {platform.name}{disabled ? ` ${t("settings.platformDisabledSuffix")}` : ""}
          </option>
        {/each}
      </select>
    </label>
  </section>
</div>

<style>
  .platforms {
    display: flex;
    flex-direction: column;
    gap: 8px;
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
