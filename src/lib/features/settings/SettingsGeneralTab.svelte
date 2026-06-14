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
  import { LANGUAGE_OPTIONS, type MessageKey, type TranslationParams } from "$lib/i18n";
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
      <span class="field-label">{t("settings.uiZoom")} - {settings.uiScalePercent}%</span>
      <input
        type="range"
        min="75"
        max="150"
        step="5"
        value={uiScale.input}
        oninput={(e) => {
          uiScale.input = (e.currentTarget as HTMLInputElement).value;
          uiScale.commit();
        }}
        class="slider-input"
      />
    </label>

    <div class="field">
      <span class="field-label">{t("settings.theme")}</span>
      <div class="theme-grid">
        {#each getAllThemes() as theme (theme.id)}
          <button
            type="button"
            class="theme-swatch"
            class:selected={settings.themeId === theme.id}
            title={theme.isCustom ? (theme.displayName ?? theme.id) : t(theme.labelKey)}
            style="--swatch-bg: rgb({theme.tokens.bgRgb}); --swatch-card: {theme.tokens.bgCard}; --swatch-fg: {theme.tokens.fg}; --swatch-border: {theme.tokens.border};"
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

    <label class="field">
      <span class="field-label">{t("settings.animations")}</span>
      <select class="text-input select-input" bind:value={settings.animations}>
        <option value="system">{t("settings.animationsSystem")}</option>
        <option value="on">{t("settings.animationsOn")}</option>
        <option value="off">{t("settings.animationsOff")}</option>
      </select>
    </label>

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
    <h3>{t("settings.integrations")}</h3>
    <ToggleSetting
      label={t("settings.deepLinks")}
      enabled={settings.deepLinksEnabled}
      accent={neutralAccent}
      onLabel={t("common.enabled")}
      offLabel={t("common.disabled")}
      onToggle={() => settings.deepLinksEnabled = !settings.deepLinksEnabled}
    />
    <p class="hint">{t("settings.deepLinksHint")}</p>
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
