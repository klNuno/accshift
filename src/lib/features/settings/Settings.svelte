<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getSettings, saveSettings, ALL_PLATFORMS } from "./store";
  import { addToast } from "../notifications/store.svelte";
  import ToggleSetting from "./ToggleSetting.svelte";
  import {
    hasApiKey,
    openSteamApiKeyPage as openSteamApiKeyPageInBrowser,
    setApiKey,
  } from "$lib/platforms/steam/steamApi";
  import { getPlatformDefinition } from "$lib/platforms/registry";
  import {
    getThemeDefinition,
    getAllThemes,
    loadCustomThemes,
    saveCustomTheme,
    deleteCustomTheme as deleteCustomThemeFromRegistry,
    parseThemeJson,
    exportThemeJson,
  } from "$lib/theme/themes";
  import {
    DEFAULT_LOCALE,
    LANGUAGE_OPTIONS,
    normalizeLocale,
    translate,
    type MessageKey,
    type TranslationParams,
  } from "$lib/i18n";
  import { hashPinCode, sanitizePinDigits } from "$lib/shared/pin";
  import type { PlatformDef } from "$lib/features/settings/types";

  type SettingsTabDef = {
    id: string;
    labelKey: MessageKey;
    accent: string;
    visible?: () => boolean;
    platformDef?: PlatformDef;
  };

  let {
    onClose,
    onPlatformsChanged,
    onSettingsUpdated = () => {},
    onRefreshAvatarsNow = async () => {},
    onRefreshBansNow = async () => {},
    runtimeOs = "unknown",
  }: {
    onClose: () => void;
    onPlatformsChanged?: () => void;
    onSettingsUpdated?: () => void;
    onRefreshAvatarsNow?: () => void | Promise<void>;
    onRefreshBansNow?: () => void | Promise<void>;
    runtimeOs?: "windows" | "linux" | "macos" | "unknown";
  } = $props();

  let settings = $state(getSettings());
  let apiKey = $state("");
  let apiKeyConfigured = $state(false);
  let apiKeyTouched = $state(false);
  let platformPaths = $state<Record<string, string>>({});
  let platformPathsKey = $derived(JSON.stringify(platformPaths));
  let showLastLoginKey = $derived(JSON.stringify(settings.accountDisplay.showLastLoginPerPlatform));
  let pinCodeInput = $state("");
  let ActivePlatformComponent = $state<any>(null);
  let uiScalePercentInput = $state("");
  let backgroundOpacityInput = $state("");
  let avatarCacheDaysInput = $state("");
  let banCheckDaysInput = $state("");
  let inactivityBlurSecondsInput = $state("");
  let avatarRefreshLoading = $state(false);
  let banRefreshLoading = $state(false);
  let hydrated = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let lastSavedToastAt = 0;
  let lastPersistedSnapshot = "";
  let lastPlatformSnapshot = "";
  let activeSettingsTab = $state<string>("general");
  let tabsRef = $state<HTMLDivElement | null>(null);
  let tabUiFrame: number | null = null;
  let tabResizeObserver: ResizeObserver | null = null;
  let tabsOverflowing = $state(false);
  let canScrollTabsLeft = $state(false);
  let canScrollTabsRight = $state(false);
  const SAVE_TOAST_COOLDOWN_MS = 1500;
  const PIN_CODE_LENGTH = 4;
  const NEUTRAL_TAB_ACCENT = "#71717a";
  const NEUTRAL_CONTROL_ACCENT = NEUTRAL_TAB_ACCENT;
  const coreTabConfig: SettingsTabDef[] = [
    { id: "general", labelKey: "settings.general", accent: NEUTRAL_TAB_ACCENT },
    { id: "platforms", labelKey: "settings.platforms", accent: NEUTRAL_TAB_ACCENT },
    { id: "privacy", labelKey: "settings.privacy", accent: NEUTRAL_TAB_ACCENT },
  ];

  let platformTabConfig = $derived.by(() => {
    return ALL_PLATFORMS
      .filter((p) => p.settingsTabKey && settings.enabledPlatforms.includes(p.id))
      .map((p): SettingsTabDef => ({
        id: `platform:${p.id}`,
        labelKey: p.settingsTabKey as MessageKey,
        accent: p.accent,
        platformDef: p,
      }));
  });

  let tabConfig = $derived([...coreTabConfig, ...platformTabConfig]);

  let visibleTabs = $derived.by(() =>
    tabConfig.filter((tab) => tab.visible ? tab.visible() : true)
  );

  let visiblePlatformOptions = $derived.by(() =>
    ALL_PLATFORMS.filter((platform) => platform.implemented || settings.enabledPlatforms.includes(platform.id))
  );

  function t(key: MessageKey, params?: TranslationParams): string {
    return translate(settings.language ?? DEFAULT_LOCALE, key, params);
  }

  function clampInt(value: number, min: number, max: number, fallback: number): number {
    if (!Number.isFinite(value)) return fallback;
    return Math.min(max, Math.max(min, Math.round(value)));
  }

  function buildPlatformSnapshot(): string {
    return JSON.stringify({
      enabledPlatforms: [...settings.enabledPlatforms].sort(),
      defaultPlatformId: settings.defaultPlatformId,
    });
  }

  function normalizeSettings() {
    settings.themeId = getThemeDefinition(settings.themeId).id;
    settings.language = normalizeLocale(settings.language);
    settings.backgroundOpacity = clampInt(settings.backgroundOpacity, 0, 100, 100);
    settings.uiScalePercent = clampInt(settings.uiScalePercent, 75, 150, 100);
    settings.suspendGraphicsWhenMinimized = settings.suspendGraphicsWhenMinimized !== false;
    settings.minimizeOnAccountSwitch = Boolean(settings.minimizeOnAccountSwitch);
    settings.dataRefresh.avatarCacheDays = clampInt(settings.dataRefresh.avatarCacheDays, 0, 90, 7);
    settings.dataRefresh.banCheckDays = clampInt(settings.dataRefresh.banCheckDays, 0, 90, 7);
    settings.inactivityBlurSeconds = clampInt(settings.inactivityBlurSeconds, 0, 3600, 60);
    settings.platformSettings.steam.launchOptions = (settings.platformSettings.steam.launchOptions || "").trim();
    if (!settings.pinEnabled) {
      settings.pinHash = "";
    }
    if (!visiblePlatformOptions.some((platform) => platform.id === settings.defaultPlatformId)) {
      settings.defaultPlatformId = "steam";
    }
    if (!settings.enabledPlatforms.length) settings.enabledPlatforms = ["steam"];
    if (!settings.enabledPlatforms.includes(settings.defaultPlatformId)) {
      settings.defaultPlatformId = settings.enabledPlatforms[0];
    }
    const selectableEnabled = settings.enabledPlatforms.filter((platformId) => isPlatformSelectable(platformId));
    if (selectableEnabled.length > 0 && !selectableEnabled.includes(settings.defaultPlatformId)) {
      settings.defaultPlatformId = selectableEnabled[0];
    }
  }

  function buildPersistSnapshot(): string {
    const pendingApiKey = apiKeyTouched ? apiKey.trim() : "";
    const trimmedPaths: Record<string, string> = {};
    for (const [id, p] of Object.entries(platformPaths)) {
      trimmedPaths[id] = p.trim();
    }
    return JSON.stringify({
      settings,
      pendingApiKey,
      apiKeyConfigured,
      platformPaths: trimmedPaths,
    });
  }

  function refreshNumericInputsFromSettings() {
    uiScalePercentInput = String(settings.uiScalePercent);
    backgroundOpacityInput = String(settings.backgroundOpacity);
    avatarCacheDaysInput = String(settings.dataRefresh.avatarCacheDays);
    banCheckDaysInput = String(settings.dataRefresh.banCheckDays);
    inactivityBlurSecondsInput = String(settings.inactivityBlurSeconds);
  }

  function commitUiScalePercent() {
    settings.uiScalePercent = clampInt(Number(uiScalePercentInput), 75, 150, settings.uiScalePercent);
    uiScalePercentInput = String(settings.uiScalePercent);
  }

  function commitBackgroundOpacity() {
    settings.backgroundOpacity = clampInt(Number(backgroundOpacityInput), 0, 100, settings.backgroundOpacity);
    backgroundOpacityInput = String(settings.backgroundOpacity);
  }

  function commitAvatarCacheDays() {
    settings.dataRefresh.avatarCacheDays = clampInt(
      Number(avatarCacheDaysInput),
      0,
      90,
      settings.dataRefresh.avatarCacheDays,
    );
    avatarCacheDaysInput = String(settings.dataRefresh.avatarCacheDays);
  }

  function commitBanCheckDays() {
    settings.dataRefresh.banCheckDays = clampInt(
      Number(banCheckDaysInput),
      0,
      90,
      settings.dataRefresh.banCheckDays,
    );
    banCheckDaysInput = String(settings.dataRefresh.banCheckDays);
  }

  function commitInactivityBlurSeconds() {
    settings.inactivityBlurSeconds = clampInt(Number(inactivityBlurSecondsInput), 0, 3600, settings.inactivityBlurSeconds);
    inactivityBlurSecondsInput = String(settings.inactivityBlurSeconds);
  }

  async function persistNow() {
    normalizeSettings();
    const sanitizedPinInput = sanitizePinDigits(pinCodeInput);
    if (settings.pinEnabled && sanitizedPinInput.length === PIN_CODE_LENGTH) {
      settings.pinHash = await hashPinCode(sanitizedPinInput);
      pinCodeInput = "";
    }

    const snapshot = buildPersistSnapshot();
    if (snapshot === lastPersistedSnapshot) return;

    const nextPlatformSnapshot = buildPlatformSnapshot();
    const platformsChanged = nextPlatformSnapshot !== lastPlatformSnapshot;
    const previousState = lastPersistedSnapshot
      ? JSON.parse(lastPersistedSnapshot) as { platformPaths?: Record<string, string> }
      : {};
    const prevPaths = previousState.platformPaths ?? {};

    saveSettings(settings);
    onSettingsUpdated?.();

    try {
      if (apiKeyTouched) {
        const trimmedApiKey = apiKey.trim();
        await setApiKey(trimmedApiKey);
        apiKeyConfigured = trimmedApiKey.length > 0;
        apiKeyTouched = false;
        apiKey = "";
      }
      for (const platformId of Object.keys(platformPaths)) {
        const nextPath = platformPaths[platformId]?.trim() ?? "";
        if ((prevPaths[platformId] ?? "") !== nextPath) {
          await invoke("platform_set_path", { platformId, path: nextPath });
        }
      }
      lastPersistedSnapshot = buildPersistSnapshot();
      lastPlatformSnapshot = nextPlatformSnapshot;
      const now = Date.now();
      if (now - lastSavedToastAt >= SAVE_TOAST_COOLDOWN_MS) {
        addToast(t("settings.saved"));
        lastSavedToastAt = now;
      }
      if (platformsChanged) {
        onPlatformsChanged?.();
      }
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  }

  function queueSave() {
    if (!hydrated) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void persistNow();
    }, 220);
  }

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
    }
  }

  function updateTabScrollState() {
    const el = tabsRef;
    if (!el) {
      tabsOverflowing = false;
      canScrollTabsLeft = false;
      canScrollTabsRight = false;
      return;
    }
    const maxScrollLeft = Math.max(0, el.scrollWidth - el.clientWidth);
    tabsOverflowing = maxScrollLeft > 6;
    canScrollTabsLeft = tabsOverflowing && el.scrollLeft > 6;
    canScrollTabsRight = tabsOverflowing && el.scrollLeft < maxScrollLeft - 6;
  }

  function scrollActiveTabIntoView(behavior: ScrollBehavior = "smooth") {
    const activeButton = tabsRef?.querySelector<HTMLElement>(`[data-settings-tab="${activeSettingsTab}"]`);
    activeButton?.scrollIntoView({ inline: "nearest", block: "nearest", behavior });
  }

  function queueTabUiRefresh(scrollActive = false) {
    if (tabUiFrame !== null) cancelAnimationFrame(tabUiFrame);
    tabUiFrame = requestAnimationFrame(() => {
      updateTabScrollState();
      if (scrollActive) {
        scrollActiveTabIntoView("auto");
      }
      tabUiFrame = null;
    });
  }

  function selectSettingsTab(tabId: string) {
    activeSettingsTab = tabId;
    loadActivePlatformComponent(tabId);
    queueTabUiRefresh(true);
  }

  function loadActivePlatformComponent(tabId: string) {
    if (!tabId.startsWith("platform:")) {
      ActivePlatformComponent = null;
      return;
    }
    const platformId = tabId.slice("platform:".length);
    const def = ALL_PLATFORMS.find((p) => p.id === platformId);
    if (def?.settingsComponent) {
      def.settingsComponent().then((mod) => {
        if (activeSettingsTab === tabId) {
          ActivePlatformComponent = mod.default;
        }
      });
    } else {
      ActivePlatformComponent = null;
    }
  }

  function scrollTabs(direction: -1 | 1) {
    const el = tabsRef;
    if (!el) return;
    el.scrollBy({
      left: Math.max(180, el.clientWidth * 0.6) * direction,
      behavior: "smooth",
    });
  }

  async function choosePlatformPath(platformId: string) {
    try {
      const selected = await invoke<string>("platform_select_path", { platformId });
      platformPaths[platformId] = selected;
    } catch {
      // User canceled the picker or the native dialog failed.
    }
  }

  async function openSteamApiKeyPage() {
    try {
      await openSteamApiKeyPageInBrowser();
    } catch {
      addToast(t("settings.openApiKeyFailed"));
    }
  }

  async function handleRefreshAvatarsNow() {
    if (avatarRefreshLoading) return;
    avatarRefreshLoading = true;
    try {
      await onRefreshAvatarsNow();
    } finally {
      avatarRefreshLoading = false;
    }
  }

  async function handleRefreshBansNow() {
    if (banRefreshLoading) return;
    banRefreshLoading = true;
    try {
      await onRefreshBansNow();
    } finally {
      banRefreshLoading = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }

  onMount(async () => {
    const enabledIds = settings.enabledPlatforms;
    const pathPromises = enabledIds.map((id) =>
      invoke<string>("platform_get_path", { platformId: id })
        .then((p: string) => [id, p] as const)
        .catch(() => [id, ""] as const)
    );
    const [apiKeyResult, ...pathResults] = await Promise.allSettled([
      hasApiKey(),
      ...pathPromises,
    ]);

    apiKeyConfigured = apiKeyResult.status === "fulfilled" ? apiKeyResult.value : false;
    apiKey = "";
    apiKeyTouched = false;
    const paths: Record<string, string> = {};
    for (const id of enabledIds) {
      paths[id] = "";
    }
    for (const result of pathResults) {
      if (result.status === "fulfilled") {
        const [id, p] = result.value as [string, string];
        paths[id] = p;
      }
    }
    platformPaths = paths;

    normalizeSettings();
    refreshNumericInputsFromSettings();
    lastPersistedSnapshot = buildPersistSnapshot();
    lastPlatformSnapshot = buildPlatformSnapshot();
    hydrated = true;

    queueTabUiRefresh(true);

    if (tabsRef && typeof ResizeObserver !== "undefined") {
      tabResizeObserver = new ResizeObserver(() => {
        updateTabScrollState();
      });
      tabResizeObserver.observe(tabsRef);
    }
  });

  onDestroy(() => {
    if (saveTimer) clearTimeout(saveTimer);
    if (tabUiFrame !== null) cancelAnimationFrame(tabUiFrame);
    tabResizeObserver?.disconnect();
  });

  $effect(() => {
    settings.dataRefresh.avatarCacheDays;
    settings.dataRefresh.banCheckDays;
    settings.inactivityBlurSeconds;
    settings.themeId;
    settings.backgroundOpacity;
    settings.suspendGraphicsWhenMinimized;
    settings.minimizeOnAccountSwitch;
    settings.language;
    settings.platformSettings.steam.runAsAdmin;
    settings.platformSettings.steam.launchOptions;
    settings.accountDisplay.showUsernames;
    settings.accountDisplay.showCardNotesInline;
    showLastLoginKey;
    settings.uiScalePercent;
    settings.defaultPlatformId;
    settings.pinEnabled;
    settings.pinHash;
    pinCodeInput;
    settings.enabledPlatforms.join(",");
    apiKey;
    apiKeyConfigured;
    apiKeyTouched;
    platformPathsKey;
    queueSave();
  });

  $effect(() => {
    const visibleIds = visibleTabs.map((tab) => tab.id);
    if (!visibleIds.includes(activeSettingsTab)) {
      activeSettingsTab = visibleIds[0] ?? "general";
      loadActivePlatformComponent(activeSettingsTab);
    }
    visibleIds.join(",");
    settings.language;
    queueTabUiRefresh(true);
  });
</script>

<svelte:window onkeydown={handleKeydown} onresize={updateTabScrollState} />

<div class="settings-panel">
  <div class="header">
    <div class="title-wrap">
      <span class="title">{t("settings.title")}</span>
    </div>
    <div class="header-actions">
      <button class="close-btn" onclick={onClose} title={t("common.close")}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    </div>
  </div>

  <div class="settings-nav-shell" class:compact={tabsOverflowing}>
    {#if tabsOverflowing}
      <button
        class="tabs-scroll-btn"
        type="button"
        onclick={() => scrollTabs(-1)}
        disabled={!canScrollTabsLeft}
        aria-label="Scroll settings tabs left"
      >
        <span>&lsaquo;</span>
      </button>
    {/if}

    <div class="settings-tabs" bind:this={tabsRef} onscroll={updateTabScrollState}>
      {#each visibleTabs as tab}
        <button
          class="settings-tab"
          class:active={activeSettingsTab === tab.id}
          type="button"
          data-settings-tab={tab.id}
          style={`--tab-accent:${tab.accent};`}
          onclick={() => selectSettingsTab(tab.id)}
        >
          <span>{t(tab.labelKey)}</span>
        </button>
      {/each}
    </div>

    {#if tabsOverflowing}
      <button
        class="tabs-scroll-btn"
        type="button"
        onclick={() => scrollTabs(1)}
        disabled={!canScrollTabsRight}
        aria-label="Scroll settings tabs right"
      >
        <span>&rsaquo;</span>
      </button>
    {/if}
  </div>

  <div class="settings-content">
    {#if activeSettingsTab === "general"}
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
            <span class="field-label">{t("settings.uiZoom")} — {settings.uiScalePercent}%</span>
            <input
              type="range"
              min="75"
              max="150"
              step="5"
              bind:value={uiScalePercentInput}
              oninput={(e) => {
                uiScalePercentInput = (e.currentTarget as HTMLInputElement).value;
                commitUiScalePercent();
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
                  <span class="swatch-preview">
                    <span class="swatch-bar"></span>
                    <span class="swatch-bar short"></span>
                  </span>
                  <span class="swatch-label">{theme.isCustom ? (theme.displayName ?? theme.id) : t(theme.labelKey)}</span>
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
            <span class="field-label">{t("settings.backgroundOpacity")} — {settings.backgroundOpacity}%</span>
            <input
              type="range"
              min="0"
              max="100"
              step="5"
              bind:value={backgroundOpacityInput}
              oninput={(e) => {
                backgroundOpacityInput = (e.currentTarget as HTMLInputElement).value;
                commitBackgroundOpacity();
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
        </section>

        <section class="card">
          <h3>{t("settings.performance")}</h3>
          <ToggleSetting
            label={t("settings.suspendGraphicsWhenMinimized")}
            enabled={settings.suspendGraphicsWhenMinimized}
            accent={NEUTRAL_CONTROL_ACCENT}
            onLabel={t("common.enabled")}
            offLabel={t("common.disabled")}
            onToggle={() => settings.suspendGraphicsWhenMinimized = !settings.suspendGraphicsWhenMinimized}
          />
          <ToggleSetting
            label={t("settings.minimizeOnAccountSwitch")}
            enabled={settings.minimizeOnAccountSwitch}
            accent={NEUTRAL_CONTROL_ACCENT}
            onLabel={t("common.enabled")}
            offLabel={t("common.disabled")}
            onToggle={() => settings.minimizeOnAccountSwitch = !settings.minimizeOnAccountSwitch}
          />
        </section>
      </div>
    {/if}

    {#if activeSettingsTab === "platforms"}
      <div class="settings-grid">
        <section class="card card-wide">
          <h3>{t("settings.platforms")}</h3>
          <div class="platforms">
            {#each visiblePlatformOptions as platform}
              {@const isSelectable = isPlatformSelectable(platform.id)}
              {@const isEnabled = settings.enabledPlatforms.includes(platform.id)}
              {@const isLocked = !isSelectable && !isEnabled}
              {@const statusLabel = platformAvailabilityLabel(platform.id)}
              <button
                class="platform-chip"
                class:disabled={isLocked}
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
                <div class="toggle" class:active={isEnabled}>
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
    {/if}

    {#if activeSettingsTab === "privacy"}
      <div class="settings-grid">
        <section class="card">
          <h3>{t("settings.privacy")}</h3>
          <label class="field">
            <span class="field-label">{t("settings.inactivityTimeout")} <span class="hint">({t("settings.zeroDisabled")})</span></span>
            <input
              type="number"
              min="0"
              max="3600"
              step="5"
              value={inactivityBlurSecondsInput}
              oninput={(e) => inactivityBlurSecondsInput = (e.currentTarget as HTMLInputElement).value}
              onblur={commitInactivityBlurSeconds}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  commitInactivityBlurSeconds();
                  (e.currentTarget as HTMLInputElement).blur();
                }
              }}
              class="text-input number-input"
            />
          </label>
        </section>

        <section class="card">
          <h3>{t("settings.security")}</h3>
          <ToggleSetting
            label={t("settings.pinLockOnAfk")}
            enabled={settings.pinEnabled}
            accent={NEUTRAL_CONTROL_ACCENT}
            onLabel={t("common.enabled")}
            offLabel={t("common.disabled")}
            onToggle={() => {
              settings.pinEnabled = !settings.pinEnabled;
              if (!settings.pinEnabled) {
                settings.pinHash = "";
                pinCodeInput = "";
              }
            }}
          />

          {#if settings.pinEnabled}
            <div class="field">
              <span class="field-label">{t("settings.pinCode")}</span>
              <input
                id="pin-code"
                type="password"
                bind:value={pinCodeInput}
                class="text-input"
                placeholder={t("settings.pinPlaceholder")}
                maxlength={PIN_CODE_LENGTH}
                inputmode="numeric"
                pattern="[0-9]*"
                oninput={(e) => pinCodeInput = sanitizePinDigits((e.currentTarget as HTMLInputElement).value)}
              />
            </div>
          {/if}
        </section>
      </div>
    {/if}

    {#if activeSettingsTab.startsWith("platform:") && ActivePlatformComponent}
      {@const platformId = activeSettingsTab.slice("platform:".length)}
      {@const platformDef = ALL_PLATFORMS.find((p) => p.id === platformId)}
      {#if platformDef && platformId in platformPaths}
        <div class="settings-grid">
          <ActivePlatformComponent
            bind:settings
            bind:path={platformPaths[platformId]}
            accent={platformDef.accent}
            {t}
            bind:apiKey
            {avatarCacheDaysInput}
            {banCheckDaysInput}
            {avatarRefreshLoading}
            {banRefreshLoading}
            onChoosePath={() => choosePlatformPath(platformId)}
            onOpenSteamApiKeyPage={openSteamApiKeyPage}
            onApiKeyInput={(value: string) => {
              apiKey = value;
              apiKeyTouched = true;
            }}
            onAvatarCacheDaysInput={(value: string) => avatarCacheDaysInput = value}
            onBanCheckDaysInput={(value: string) => banCheckDaysInput = value}
            onCommitAvatarCacheDays={commitAvatarCacheDays}
            onCommitBanCheckDays={commitBanCheckDays}
            onRefreshAvatarsNow={handleRefreshAvatarsNow}
            onRefreshBansNow={handleRefreshBansNow}
            pathLabelKey={platformDef.pathLabelKey}
            pathPlaceholder={platformDef.pathPlaceholder}
          />
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .settings-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow: hidden;
    animation: fadeIn 140ms ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(4px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--border);
  }

  .title-wrap {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title {
    font-size: 14px;
    font-weight: 700;
    color: var(--fg);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-right: 8px;
  }

  .close-btn {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
  }

  .close-btn:hover {
    background: var(--bg-muted);
    color: var(--fg);
  }

  .settings-nav-shell {
    display: grid;
    grid-template-columns: 1fr;
    align-items: center;
    gap: 10px;
  }

  .settings-nav-shell.compact {
    grid-template-columns: auto minmax(0, 1fr) auto;
  }

  .tabs-scroll-btn {
    width: 28px;
    height: 28px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-card) 92%, #fff 8%);
    color: var(--fg);
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out;
  }

  .tabs-scroll-btn:hover {
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 84%, #fff 16%);
  }

  .tabs-scroll-btn:disabled {
    opacity: 0.38;
    cursor: not-allowed;
  }

  .settings-tabs {
    display: flex;
    align-items: center;
    gap: 8px;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
    padding-bottom: 2px;
  }

  .settings-tabs::-webkit-scrollbar {
    display: none;
  }

  .settings-tab {
    flex: 0 0 auto;
    min-width: fit-content;
    border: 1px solid color-mix(in srgb, var(--tab-accent) 28%, var(--border));
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-card) 88%, #000 12%);
    color: var(--fg-muted);
    padding: 9px 14px;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    transition: transform 120ms ease-out, border-color 120ms ease-out, background 120ms ease-out, color 120ms ease-out;
  }

  .settings-tab:hover {
    color: var(--fg);
    border-color: color-mix(in srgb, var(--tab-accent) 40%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 76%, #fff 24%);
  }

  .settings-tab.active {
    color: var(--fg);
    border-color: color-mix(in srgb, var(--tab-accent) 65%, var(--border));
    background: linear-gradient(
      135deg,
      color-mix(in srgb, var(--tab-accent) 18%, var(--bg-card)),
      color-mix(in srgb, var(--bg-card) 82%, #fff 18%)
    );
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--tab-accent) 18%, transparent);
  }

  .settings-content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding-right: 4px;
    padding-bottom: 8px;
  }

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 12px;
    min-width: 0;
  }

  .card {
    background: color-mix(in srgb, var(--bg-card) 84%, #000 16%);
    border: 1px solid color-mix(in srgb, var(--border) 80%, #fff 20%);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .card-wide {
    grid-column: span 2;
  }

  .card h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 700;
    color: var(--fg);
  }

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

  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .field-label {
    font-size: 12px;
    color: var(--fg-muted);
  }

  .hint {
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .text-input {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-solid);
    color: var(--fg);
    font-size: 12px;
    padding: 9px 10px;
    outline: none;
  }

  .text-input:focus {
    border-color: color-mix(in srgb, var(--fg-muted) 55%, var(--border));
  }

  .select-input {
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    padding-right: 34px;
    background-image:
      linear-gradient(45deg, transparent 50%, var(--fg-muted) 50%),
      linear-gradient(135deg, var(--fg-muted) 50%, transparent 50%);
    background-position:
      calc(100% - 18px) calc(50% - 1px),
      calc(100% - 13px) calc(50% - 1px);
    background-size: 5px 5px, 5px 5px;
    background-repeat: no-repeat;
  }

  .number-input {
    width: 100%;
  }

  .slider-input {
    width: 100%;
    accent-color: var(--fg);
  }

  @media (max-width: 980px) {
    .card-wide {
      grid-column: span 1;
    }
  }

  .theme-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(86px, 1fr));
    gap: 8px;
    margin-top: 4px;
  }

  .theme-swatch {
    position: relative;
    border-radius: 8px;
    border: 2px solid transparent;
    background: var(--swatch-bg);
    cursor: pointer;
    padding: 8px 8px 6px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    transition: border-color 120ms ease-out, box-shadow 120ms ease-out;
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

  @media (max-width: 720px) {
    .settings-panel {
      gap: 10px;
    }

    .settings-grid {
      grid-template-columns: 1fr;
    }

    .settings-tab {
      padding: 8px 12px;
      font-size: 11px;
    }
  }
</style>
