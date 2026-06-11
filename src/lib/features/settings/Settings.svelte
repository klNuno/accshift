<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getSettings, saveSettings, ALL_PLATFORMS } from "./store";
  import { addToast } from "../notifications/store.svelte";
  import {
    hasApiKey,
    openSteamApiKeyPage as openSteamApiKeyPageInBrowser,
    setApiKey,
  } from "$lib/platforms/steam/steamApi";
  import { getPlatformDefinition } from "$lib/platforms/registry";
  import { getThemeDefinition } from "$lib/theme/themes";
  import {
    DEFAULT_LOCALE,
    normalizeLocale,
    translate,
    type MessageKey,
    type TranslationParams,
  } from "$lib/i18n";
  import { hashPinCode, sanitizePinDigits } from "$lib/shared/pin";
  import { trackDependencies } from "$lib/shared/trackDependencies";
  import { createNumericInput, clampInt } from "$lib/shared/useNumericInput.svelte";
  import type { PlatformDef } from "$lib/features/settings/types";
  import { resolvePathPlaceholder } from "$lib/features/settings/types";
  import { createSettingsTabBar, type SettingsTabDef } from "./useSettingsTabBar.svelte";
  import SettingsGeneralTab from "./SettingsGeneralTab.svelte";
  import SettingsPlatformsTab from "./SettingsPlatformsTab.svelte";
  import SettingsPrivacyTab from "./SettingsPrivacyTab.svelte";

  let {
    onClose,
    onPlatformsChanged,
    onSettingsUpdated = () => {},
    onRefreshAvatarsNow = async () => {},
    onRefreshBansNow = async () => {},
    onAccountAdded = () => {},
    runtimeOs = "unknown",
  }: {
    onClose: () => void;
    onPlatformsChanged?: () => void;
    onSettingsUpdated?: () => void;
    onRefreshAvatarsNow?: () => void | Promise<void>;
    onRefreshBansNow?: () => void | Promise<void>;
    onAccountAdded?: () => void;
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
  const uiScale = createNumericInput(() => settings.uiScalePercent, (v) => { settings.uiScalePercent = v; }, 75, 150);
  const bgOpacity = createNumericInput(() => settings.backgroundOpacity, (v) => { settings.backgroundOpacity = v; }, 0, 100);
  const avatarCacheDays = createNumericInput(() => settings.dataRefresh.avatarCacheDays, (v) => { settings.dataRefresh.avatarCacheDays = v; }, 0, 90);
  const banCheckDays = createNumericInput(() => settings.dataRefresh.banCheckDays, (v) => { settings.dataRefresh.banCheckDays = v; }, 0, 90);
  const inactivityBlur = createNumericInput(() => settings.inactivityBlurSeconds, (v) => { settings.inactivityBlurSeconds = v; }, 0, 3600);
  let avatarRefreshLoading = $state(false);
  let banRefreshLoading = $state(false);
  let hydrated = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let lastSavedToastAt = 0;
  let lastPersistedSnapshot = "";
  let lastPlatformSnapshot = "";
  let ActivePlatformComponent = $state<any>(null);
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
      .map((p): SettingsTabDef & { platformDef?: PlatformDef } => ({
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

  const tabBar = createSettingsTabBar({
    getVisibleTabs: () => visibleTabs,
    onTabSelected: loadActivePlatformComponent,
  });

  function t(key: MessageKey, params?: TranslationParams): string {
    return translate(settings.language ?? DEFAULT_LOCALE, key, params);
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
    const visiblePlatforms = ALL_PLATFORMS.filter((p) => p.implemented || settings.enabledPlatforms.includes(p.id));
    if (!visiblePlatforms.some((platform) => platform.id === settings.defaultPlatformId)) {
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
    uiScale.refresh();
    bgOpacity.refresh();
    avatarCacheDays.refresh();
    banCheckDays.refresh();
    inactivityBlur.refresh();
  }

  async function persistNow() {
    normalizeSettings();
    const sanitizedPinInput = sanitizePinDigits(pinCodeInput);
    if (settings.pinEnabled && sanitizedPinInput.length === PIN_CODE_LENGTH) {
      settings.pinHash = await hashPinCode(sanitizedPinInput);
      pinCodeInput = "";
    }

    // Capture the snapshot before any await below so edits made while persisting
    // stay dirty and get picked up by the next debounced save.
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
      lastPersistedSnapshot = snapshot;
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

  function loadActivePlatformComponent(tabId: string) {
    if (!tabId.startsWith("platform:")) {
      ActivePlatformComponent = null;
      return;
    }
    const platformId = tabId.slice("platform:".length);
    const def = ALL_PLATFORMS.find((p) => p.id === platformId);
    if (def?.settingsComponent) {
      def.settingsComponent().then((mod) => {
        if (tabBar.activeTab === tabId) {
          ActivePlatformComponent = mod.default;
        }
      });
    } else {
      ActivePlatformComponent = null;
    }
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

    tabBar.startObserver();
  });

  onDestroy(() => {
    if (saveTimer) clearTimeout(saveTimer);
    tabBar.destroy();
  });

  $effect(() => {
    trackDependencies(
      settings.dataRefresh.avatarCacheDays,
      settings.dataRefresh.banCheckDays,
      settings.inactivityBlurSeconds,
      settings.themeId,
      settings.backgroundOpacity,
      settings.animations,
      settings.suspendGraphicsWhenMinimized,
      settings.minimizeOnAccountSwitch,
      settings.language,
      settings.platformSettings.steam.runAsAdmin,
      settings.platformSettings.steam.launchOptions,
      settings.platformSettings.steam.shutdownMode,
      settings.accountDisplay.showUsernames,
      settings.accountDisplay.showCardNotesInline,
      settings.accountDisplay.expandedFolders,
      showLastLoginKey,
      settings.uiScalePercent,
      settings.defaultPlatformId,
      settings.pinEnabled,
      settings.pinHash,
      pinCodeInput,
      settings.enabledPlatforms.join(","),
      apiKey,
      apiKeyConfigured,
      apiKeyTouched,
      platformPathsKey,
    );
    queueSave();
  });

  $effect(() => {
    trackDependencies(visibleTabs.map((tab) => tab.id).join(","), settings.language);
    tabBar.ensureActiveVisible();
  });
</script>

<svelte:window onkeydown={handleKeydown} onresize={tabBar.updateScrollState} />

<div class="settings-panel">
  <div class="header">
    <div class="header-actions">
      <button class="close-btn" onclick={onClose} title={t("common.close")}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    </div>
    <div class="title-wrap">
      <span class="title">{t("settings.title")}</span>
    </div>
  </div>

  <div class="settings-nav-shell" class:compact={tabBar.tabsOverflowing}>
    {#if tabBar.tabsOverflowing}
      <button
        class="tabs-scroll-btn"
        type="button"
        onclick={() => tabBar.scroll(-1)}
        disabled={!tabBar.canScrollLeft}
        aria-label="Scroll settings tabs left"
      >
        <span>&lsaquo;</span>
      </button>
    {/if}

    <div class="settings-tabs" bind:this={tabBar.tabsRef} onscroll={tabBar.updateScrollState}>
      {#each visibleTabs as tab}
        <button
          class="settings-tab"
          class:active={tabBar.activeTab === tab.id}
          type="button"
          data-settings-tab={tab.id}
          style={`--tab-accent:${tab.accent};`}
          onclick={() => tabBar.select(tab.id)}
        >
          <span>{t(tab.labelKey)}</span>
        </button>
      {/each}
    </div>

    {#if tabBar.tabsOverflowing}
      <button
        class="tabs-scroll-btn"
        type="button"
        onclick={() => tabBar.scroll(1)}
        disabled={!tabBar.canScrollRight}
        aria-label="Scroll settings tabs right"
      >
        <span>&rsaquo;</span>
      </button>
    {/if}
  </div>

  <div class="settings-content">
    {#if tabBar.activeTab === "general"}
      <SettingsGeneralTab
        bind:settings
        {t}
        {uiScale}
        {bgOpacity}
        neutralAccent={NEUTRAL_CONTROL_ACCENT}
      />
    {/if}

    {#if tabBar.activeTab === "platforms"}
      <SettingsPlatformsTab
        bind:settings
        bind:platformPaths
        {t}
        {runtimeOs}
      />
    {/if}

    {#if tabBar.activeTab === "privacy"}
      <SettingsPrivacyTab
        bind:settings
        bind:pinCodeInput
        {t}
        {inactivityBlur}
        neutralAccent={NEUTRAL_CONTROL_ACCENT}
      />
    {/if}

    {#if tabBar.activeTab.startsWith("platform:") && ActivePlatformComponent}
      {@const platformId = tabBar.activeTab.slice("platform:".length)}
      {@const platformDef = ALL_PLATFORMS.find((p) => p.id === platformId)}
      {#if platformDef && platformId in platformPaths}
        <div class="settings-grid">
          <ActivePlatformComponent
            bind:settings
            bind:path={platformPaths[platformId]}
            accent={platformDef.accent}
            {t}
            bind:apiKey
            avatarCacheDaysInput={avatarCacheDays.input}
            banCheckDaysInput={banCheckDays.input}
            {avatarRefreshLoading}
            {banRefreshLoading}
            onChoosePath={() => choosePlatformPath(platformId)}
            onOpenSteamApiKeyPage={openSteamApiKeyPage}
            onApiKeyInput={(value: string) => {
              apiKey = value;
              apiKeyTouched = true;
            }}
            onAvatarCacheDaysInput={(value: string) => { avatarCacheDays.input = value; }}
            onBanCheckDaysInput={(value: string) => { banCheckDays.input = value; }}
            onCommitAvatarCacheDays={avatarCacheDays.commit}
            onCommitBanCheckDays={banCheckDays.commit}
            onRefreshAvatarsNow={handleRefreshAvatarsNow}
            onRefreshBansNow={handleRefreshBansNow}
            onAccountAdded={onAccountAdded}
            pathLabelKey={platformDef.pathLabelKey}
            pathPlaceholder={resolvePathPlaceholder(platformDef.pathPlaceholder, runtimeOs)}
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
    animation: page-entrance var(--motion-page-entrance) ease-out;
  }

  :global(html[data-motion="reduced"]) .settings-panel {
    animation: none;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: flex-start;
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

  .settings-panel :global(.settings-grid) {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 12px;
    min-width: 0;
  }

  .settings-panel :global(.card) {
    background: color-mix(in srgb, var(--bg-card) 84%, #000 16%);
    border: 1px solid color-mix(in srgb, var(--border) 80%, #fff 20%);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .settings-panel :global(.card-wide) {
    grid-column: span 2;
  }

  .settings-panel :global(.card h3) {
    margin: 0;
    font-size: 13px;
    font-weight: 700;
    color: var(--fg);
  }

  .settings-panel :global(.field) {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .settings-panel :global(.field-label) {
    font-size: 12px;
    color: var(--fg-muted);
  }

  .settings-panel :global(.hint) {
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .settings-panel :global(.text-input) {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-solid);
    color: var(--fg);
    font-size: 12px;
    padding: 9px 10px;
    outline: none;
  }

  .settings-panel :global(.text-input:focus) {
    border-color: color-mix(in srgb, var(--fg-muted) 55%, var(--border));
  }

  .settings-panel :global(.select-input) {
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

  .settings-panel :global(.number-input) {
    width: 100%;
  }

  .settings-panel :global(.slider-input) {
    width: 100%;
    accent-color: var(--fg);
  }

  @media (max-width: 980px) {
    .settings-panel :global(.card-wide) {
      grid-column: span 1;
    }
  }

  @media (max-width: 720px) {
    .settings-panel {
      gap: 10px;
    }

    .settings-panel :global(.settings-grid) {
      grid-template-columns: 1fr;
    }

    .settings-tab {
      padding: 8px 12px;
      font-size: 11px;
    }
  }
</style>
