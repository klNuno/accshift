<script lang="ts">
  import {
    getThemeDefinition,
    getAllThemes,
    loadCustomThemes,
    saveCustomTheme,
    deleteCustomTheme as deleteCustomThemeFromRegistry,
    parseThemeJson,
    exportThemeJson,
  } from "$lib/theme/themes";
  import type { AppThemeDefinition } from "$lib/theme/themes";
  import { LANGUAGE_OPTIONS, type MessageKey, type TranslationParams } from "$lib/i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { addToast } from "../notifications/store.svelte";
  import ToggleSetting from "./ToggleSetting.svelte";
  import type { AppSettings } from "./types";

  let {
    settings = $bindable(),
    t,
    uiScale,
    bgOpacity,
    neutralAccent,
  }: {
    settings: AppSettings;
    t: (key: MessageKey, params?: TranslationParams) => string;
    uiScale: { input: string; commit: () => void };
    bgOpacity: { input: string; commit: () => void };
    neutralAccent: string;
  } = $props();

  const INTEGRATIONS_WIKI_URL = "https://github.com/klNuno/accshift/wiki/Settings";

  // Glass themes run a fixed, tuned window fill (see themes.ts); the slider
  // only applies to regular themes and just bred broken combinations on glass.
  let isGlassTheme = $derived(Boolean(getThemeDefinition(settings.themeId).glass));

  // Glass themes carry translucent surfaces (liquid glass's cards are white
  // at ~13% alpha): painted opaque on the swatch they turn into a white box.
  // The swatch simulates the real stack instead - a fake blurred desktop
  // under the theme's window veil, with the card at a preview alpha.
  function swatchStyle(theme: AppThemeDefinition): string {
    if (!theme.glass) {
      return `--swatch-bg: rgb(${theme.tokens.bgRgb}); --swatch-card: ${theme.tokens.bgCard}; --swatch-fg: ${theme.tokens.fg}; --swatch-border: ${theme.tokens.border};`;
    }
    const isLiquid = theme.id === "liquid-glass";
    const veil = isLiquid ? 0.2 : 0.6;
    const cardPct = isLiquid ? 22 : 72;
    const bg = `linear-gradient(rgb(${theme.tokens.bgRgb} / ${veil}), rgb(${theme.tokens.bgRgb} / ${veil})), linear-gradient(135deg, #8a63a8, #4f6d9e 48%, #a8825f)`;
    const card = `color-mix(in srgb, ${theme.tokens.bgCard} ${cardPct}%, transparent)`;
    const border = `color-mix(in srgb, ${theme.tokens.border} 45%, transparent)`;
    return `--swatch-bg: ${bg}; --swatch-card: ${card}; --swatch-fg: ${theme.tokens.fg}; --swatch-border: ${border};`;
  }

  async function openIntegrationsWiki() {
    try {
      await invoke("open_url", { url: INTEGRATIONS_WIKI_URL });
    } catch {
      addToast(t("settings.openHelpFailed"), { type: "error" });
    }
  }
</script>

<div class="settings-grid">
  <section class="card">
    <h3>{t("settings.appearance")}</h3>
    <label class="field">
      <span class="field-label">{t("settings.language")}</span>
      <select class="text-input select-input" bind:value={settings.language}>
        {#each LANGUAGE_OPTIONS as option}
          <option value={option.code}>{t(option.labelKey)}</option>
        {/each}
      </select>
    </label>

    <label class="field">
      <span class="field-label">{t("settings.uiZoom")} - {uiScale.input}%</span>
      <!-- Zoom relayouts the whole app: preview the value while dragging, apply on release. -->
      <input
        type="range"
        min="75"
        max="150"
        step="5"
        value={uiScale.input}
        oninput={(e) => {
          uiScale.input = (e.currentTarget as HTMLInputElement).value;
        }}
        onchange={(e) => {
          uiScale.input = (e.currentTarget as HTMLInputElement).value;
          uiScale.commit();
        }}
        class="slider-input"
      />
    </label>

    <label class="field">
      <span class="field-label">{t("settings.animations")}</span>
      <select class="text-input select-input" bind:value={settings.animations}>
        <option value="system">{t("settings.animationsSystem")}</option>
        <option value="on">{t("settings.animationsOn")}</option>
        <option value="off">{t("settings.animationsOff")}</option>
      </select>
    </label>
  </section>

  <section class="card">
    <h3>{t("settings.theme")}</h3>
    <div class="field">
      <div class="theme-grid">
        {#each getAllThemes() as theme (theme.id)}
          <button
            type="button"
            class="theme-swatch"
            class:selected={settings.themeId === theme.id}
            title={theme.isCustom ? (theme.displayName ?? theme.id) : t(theme.labelKey)}
            style={swatchStyle(theme)}
            onclick={() => settings.themeId = theme.id}
          >
            <span class="swatch-inner">
              <span class="swatch-preview">
                <span class="swatch-bar"></span>
                <span class="swatch-bar short"></span>
              </span>
              <span class="swatch-label">{theme.isCustom ? (theme.displayName ?? theme.id) : t(theme.labelKey)}</span>
            </span>
            {#if theme.isCustom}
              <!-- svelte-ignore node_invalid_placement_ssr -->
              <button
                type="button"
                class="swatch-delete"
                title={t("settings.themeDelete")}
                onclick={(e: MouseEvent) => {
                  e.stopPropagation();
                  void (async () => {
                    await deleteCustomThemeFromRegistry(theme.id);
                    if (settings.themeId === theme.id) settings.themeId = "dark";
                    addToast(t("settings.themeDeleted"));
                  })();
                }}
              >&times;</button>
            {/if}
          </button>
        {/each}
      </div>
      <div class="theme-actions-row">
        <button type="button" class="theme-action-btn" onclick={async () => {
          try {
            const json = await navigator.clipboard.readText();
            const parsed = parseThemeJson(json);
            if (!parsed) { addToast(t("settings.themeInvalidJson")); return; }
            await saveCustomTheme(parsed);
            await loadCustomThemes();
            settings.themeId = parsed.id;
            addToast(t("settings.themeImported"));
          } catch { addToast(t("settings.themeInvalidJson")); }
        }}>{t("settings.themeImport")}</button>
        <button type="button" class="theme-action-btn" onclick={() => {
          const theme = getThemeDefinition(settings.themeId);
          navigator.clipboard.writeText(exportThemeJson(theme));
          addToast(t("settings.themeExportCopied"));
        }}>{t("settings.themeExport")}</button>
      </div>
    </div>

    {#if !isGlassTheme}
      <label class="field">
        <span class="field-label">{t("settings.backgroundOpacity")} - {settings.backgroundOpacity}%</span>
        <input
          type="range"
          min="0"
          max="100"
          step="5"
          value={bgOpacity.input}
          oninput={(e) => {
            bgOpacity.input = (e.currentTarget as HTMLInputElement).value;
            bgOpacity.commit();
          }}
          class="slider-input"
        />
      </label>
    {/if}
  </section>

  <section class="card">
    <h3>{t("settings.accountDisplay")}</h3>
    <ToggleSetting
      label={t("settings.showNotesUnderCards")}
      enabled={settings.accountDisplay.showCardNotesInline}
      onLabel={t("common.inline")}
      offLabel={t("common.tooltip")}
      onToggle={() => settings.accountDisplay.showCardNotesInline = !settings.accountDisplay.showCardNotesInline}
    />
    <ToggleSetting
      label={t("settings.expandedFolders")}
      enabled={settings.accountDisplay.expandedFolders}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => settings.accountDisplay.expandedFolders = !settings.accountDisplay.expandedFolders}
    />
    <ToggleSetting
      label={t("settings.cardColorOutlines")}
      description={t("settings.cardColorOutlinesHint")}
      enabled={settings.accountDisplay.cardColorOutlines}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => settings.accountDisplay.cardColorOutlines = !settings.accountDisplay.cardColorOutlines}
    />
  </section>

  <section class="card">
    <h3>{t("settings.performance")}</h3>
    <ToggleSetting
      label={t("settings.suspendGraphicsWhenMinimized")}
      enabled={settings.suspendGraphicsWhenMinimized}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => settings.suspendGraphicsWhenMinimized = !settings.suspendGraphicsWhenMinimized}
    />
    <ToggleSetting
      label={t("settings.minimizeOnAccountSwitch")}
      enabled={settings.minimizeOnAccountSwitch}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => settings.minimizeOnAccountSwitch = !settings.minimizeOnAccountSwitch}
    />
  </section>

  <section class="card">
    <div class="card-title-row">
      <h3>{t("settings.integrations")}</h3>
      <button
        type="button"
        class="wiki-btn"
        onclick={openIntegrationsWiki}
        title={t("settings.help")}
        aria-label={t("settings.help")}
      >?</button>
    </div>
    <ToggleSetting
      label={t("settings.deepLinks")}
      description={t("settings.deepLinksHint")}
      enabled={settings.deepLinksEnabled}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => settings.deepLinksEnabled = !settings.deepLinksEnabled}
    />
    <ToggleSetting
      label={t("settings.cliEnabled")}
      description={t("settings.cliEnabledHint")}
      enabled={settings.cliEnabled}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => settings.cliEnabled = !settings.cliEnabled}
    />
  </section>
</div>

<style>
  .theme-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(86px, 1fr));
    gap: 8px;
    margin-top: 4px;
  }

  /* WebKit (Safari/macOS WebView) ignores `display: flex` on <button>, so the
     swatch button stays as a block container and an inner span holds the
     flex layout. See https://bugs.webkit.org/show_bug.cgi?id=147068. */
  .theme-swatch {
    position: relative;
    box-sizing: border-box;
    width: 100%;
    border-radius: 8px;
    border: 2px solid transparent;
    background: var(--swatch-bg);
    cursor: pointer;
    padding: 8px 8px 6px;
    transition: border-color 120ms ease-out, box-shadow 120ms ease-out;
  }

  .swatch-inner {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }

  .theme-swatch:hover {
    border-color: color-mix(in srgb, var(--swatch-fg) 25%, transparent);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.18);
  }

  .theme-swatch.selected {
    border-color: var(--swatch-fg);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--swatch-fg) 30%, transparent);
  }

  .swatch-preview {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 6px;
    width: 100%;
    box-sizing: border-box;
    border-radius: 4px;
    background: var(--swatch-card);
    border: 1px solid var(--swatch-border);
  }

  .swatch-bar {
    height: 4px;
    border-radius: 2px;
    background: var(--swatch-fg);
    opacity: 0.35;
  }

  .swatch-bar.short {
    width: 60%;
    opacity: 0.2;
  }

  .swatch-label {
    font-size: 10px;
    font-weight: 600;
    color: var(--swatch-fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: center;
    line-height: 1.2;
    opacity: 0.7;
  }

  .theme-swatch.selected .swatch-label {
    opacity: 1;
  }

  .swatch-delete {
    position: absolute;
    top: 2px;
    right: 4px;
    font-size: 14px;
    line-height: 1;
    color: var(--swatch-fg);
    opacity: 0;
    cursor: pointer;
    padding: 2px 4px;
    border: none;
    background: color-mix(in srgb, var(--swatch-bg) 80%, #000 20%);
    border-radius: 4px;
  }

  .theme-swatch:hover .swatch-delete {
    opacity: 0.6;
  }

  .swatch-delete:hover {
    opacity: 1 !important;
  }

  .theme-actions-row {
    display: flex;
    gap: 8px;
    margin-top: 4px;
  }

  .card-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-bottom: 8px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 65%, transparent);
  }

  /* The row carries the header divider so it spans past the ? button. */
  .card-title-row h3 {
    margin: 0;
    padding-bottom: 0;
    border-bottom: none;
  }

  .wiki-btn {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--fg-subtle);
    font-size: 11px;
    font-weight: 700;
    cursor: pointer;
    transition: border-color 120ms ease-out, color 120ms ease-out;
  }

  .wiki-btn:hover {
    color: var(--fg);
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
  }

  .theme-action-btn {
    flex: 1;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--fg-muted);
    font-size: 11px;
    padding: 5px 12px;
    cursor: pointer;
    transition: background 120ms ease-out;
  }

  .theme-action-btn:hover {
    background: var(--bg-card-hover);
    color: var(--fg);
  }
</style>
