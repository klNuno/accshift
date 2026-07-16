<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
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
  import type { PlatformDef } from "$lib/shared/platform";
  import { resolvePathPlaceholder } from "$lib/shared/platform";
  import { createSettingsTabBar, type SettingsTabDef } from "./useSettingsTabBar.svelte";
  import SettingsGeneralTab from "./SettingsGeneralTab.svelte";
  import SettingsPlatformsTab from "./SettingsPlatformsTab.svelte";
  import SettingsPrivacyTab from "./SettingsPrivacyTab.svelte";
  import type { AppSettings } from "./types";

  let {
    onClose,
    onPlatformsChanged,
    onSettingsUpdated = () => {},
    onRefreshAvatarsNow = async () => {},
    onRefreshBansNow = async () => {},
    onAccountAdded = () => {},
    onReplayOnboarding = () => {},
    runtimeOs = "unknown",
    registerSearchFocus = () => {},
    registerFlush = () => {},
  }: {
    onClose: () => void | Promise<void>;
    onPlatformsChanged?: () => void;
    onSettingsUpdated?: () => void;
    onRefreshAvatarsNow?: () => void | Promise<void>;
    onRefreshBansNow?: () => void | Promise<void>;
    onAccountAdded?: () => void;
    onReplayOnboarding?: () => void;
    runtimeOs?: "windows" | "linux" | "macos" | "unknown";
    registerSearchFocus?: (fn: (() => void) | null) => void;
    registerFlush?: (fn: (() => Promise<void>) | null) => void;
  } = $props();

  let settings = $state(getSettings());
  let apiKey = $state("");
  let apiKeyConfigured = $state(false);
  let apiKeyTouched = $state(false);
  let apiKeyError = $state(false);
  let platformPaths = $state<Record<string, string>>({});
  let platformPathErrors = $state<Record<string, boolean>>({});
  let platformPathsKey = $derived(JSON.stringify(platformPaths));
  let showLastLoginKey = $derived(JSON.stringify(settings.accountDisplay.showLastLoginPerPlatform));
  let healthCheckKey = $derived(JSON.stringify(settings.healthCheckPerPlatform));
  let pinCodeInput = $state("");
  let pinSetupPending = $state(false);
  const uiScale = createNumericInput(() => settings.uiScalePercent, (v) => { settings.uiScalePercent = v; }, 75, 150);
  const bgOpacity = createNumericInput(() => settings.backgroundOpacity, (v) => { settings.backgroundOpacity = v; }, 0, 100);
  const avatarCacheDays = createNumericInput(() => settings.dataRefresh.avatarCacheDays, (v) => { settings.dataRefresh.avatarCacheDays = v; }, 0, 90);
  const banCheckDays = createNumericInput(() => settings.dataRefresh.banCheckDays, (v) => { settings.dataRefresh.banCheckDays = v; }, 0, 90);
  const inactivityBlur = createNumericInput(() => settings.inactivityBlurSeconds, (v) => { settings.inactivityBlurSeconds = v; }, 0, 3600);
  let avatarRefreshLoading = $state(false);
  let banRefreshLoading = $state(false);
  let hydrated = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let persistChain: Promise<void> = Promise.resolve();
  let resolveHydrationReady!: () => void;
  const hydrationReady = new Promise<void>((resolve) => {
    resolveHydrationReady = resolve;
  });
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

  let visibleCoreTabs = $derived(visibleTabs.filter((tab) => !tab.id.startsWith("platform:")));
  let visiblePlatformTabs = $derived(
    visibleTabs.filter((tab) => tab.id.startsWith("platform:")) as (SettingsTabDef & { platformDef?: PlatformDef })[],
  );
  // Vertical strokes for the three core sections; platform entries use their accent dot.
  const CORE_TAB_ICONS: Record<string, string> = {
    general: "M4 21v-7 M4 10V3 M12 21v-9 M12 8V3 M20 21v-5 M20 12V3 M1 14h6 M9 8h6 M17 16h6",
    platforms: "M3 3h7v7H3z M14 3h7v7h-7z M14 14h7v7h-7z M3 14h7v7H3z",
    privacy: "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z",
  };

  const tabBar = createSettingsTabBar({
    getVisibleTabs: () => visibleTabs,
    onTabSelected: loadActivePlatformComponent,
  });

  let activeTabLabelKey = $derived(
    visibleTabs.find((tab) => tab.id === tabBar.activeTab)?.labelKey ?? ("settings.title" as MessageKey),
  );

  let contentRef = $state<HTMLDivElement | null>(null);
  let platformSearchInput: HTMLInputElement | null = null;

  function selectTab(tabId: string) {
    tabBar.select(tabId);
    // A leftover scroll offset from the previous section is disorienting.
    if (contentRef) contentRef.scrollTop = 0;
  }

  /** mod+f while settings are open: jump to the Platforms tab search. */
  async function focusPlatformSearch() {
    if (tabBar.activeTab !== "platforms") selectTab("platforms");
    await tick();
    platformSearchInput?.focus();
    platformSearchInput?.select();
  }

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

  function buildPersistSnapshot(settingsValue: AppSettings = settings): string {
    const pendingApiKey = apiKeyTouched ? apiKey.trim() : "";
    const trimmedPaths: Record<string, string> = {};
    for (const [id, p] of Object.entries(platformPaths)) {
      trimmedPaths[id] = p.trim();
    }
    return JSON.stringify({
      settings: settingsValue,
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

  async function persistCurrentState() {
    normalizeSettings();
    const sanitizedPinInput = sanitizePinDigits(pinCodeInput);
    const pinRequested = settings.pinEnabled || pinSetupPending;
    let pinCommitted = false;
    if (pinRequested && sanitizedPinInput.length === PIN_CODE_LENGTH) {
      const nextPinHash = await hashPinCode(sanitizedPinInput);
      if (settings.pinEnabled || pinSetupPending) {
        settings.pinHash = nextPinHash;
        settings.pinEnabled = true;
        pinSetupPending = false;
        pinCommitted = true;
        if (sanitizePinDigits(pinCodeInput) === sanitizedPinInput) {
          pinCodeInput = "";
        }
      }
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
    if (pinCommitted) {
      addToast(t("settings.pinSaved"), { type: "success" });
    }

    let hadError = false;
    if (apiKeyTouched) {
      const trimmedApiKey = apiKey.trim();
      if (trimmedApiKey.length > 0) {
        try {
          await setApiKey(trimmedApiKey);
          apiKeyConfigured = true;
          apiKeyTouched = false;
          apiKey = "";
          apiKeyError = false;
        } catch (e) {
          console.error("Failed to save Steam API key:", e);
          apiKeyError = true;
          hadError = true;
          addToast(t("settings.apiKeySaveFailed"), { type: "error" });
        }
      } else {
        // An emptied input is not a delete request. Removing the stored key
        // only happens through the explicit clear button (clearApiKey).
        apiKeyTouched = false;
      }
    }

    for (const platformId of Object.keys(platformPaths)) {
      const nextPath = platformPaths[platformId]?.trim() ?? "";
      if ((prevPaths[platformId] ?? "") !== nextPath) {
        try {
          await invoke("platform_set_path", { platformId, path: nextPath });
          platformPathErrors[platformId] = false;
        } catch (e) {
          console.error(`Failed to save ${platformId} path:`, e);
          platformPathErrors[platformId] = true;
          hadError = true;
          const platformName = getPlatformDefinition(platformId)?.name ?? platformId;
          addToast(t("settings.pathSaveFailed", { platform: platformName }), { type: "error" });
        }
      }
    }

    lastPlatformSnapshot = nextPlatformSnapshot;
    if (platformsChanged) {
      onPlatformsChanged?.();
    }
    // Leave lastPersistedSnapshot stale on failure so the next edit retries.
    if (hadError) return;
    lastPersistedSnapshot = snapshot;
    const now = Date.now();
    if (now - lastSavedToastAt >= SAVE_TOAST_COOLDOWN_MS) {
      addToast(t("settings.saved"));
      lastSavedToastAt = now;
    }
  }

  function persistNow(): Promise<void> {
    const next = persistChain.catch(() => {}).then(persistCurrentState);
    persistChain = next;
    return next;
  }

  function flushSettingsNow(): Promise<void> {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
    }
    return persistNow();
  }

  async function clearApiKey() {
    try {
      await setApiKey("");
      apiKeyConfigured = false;
      apiKey = "";
      apiKeyTouched = false;
      apiKeyError = false;
      addToast(t("settings.apiKeyCleared"));
    } catch (e) {
      console.error("Failed to clear Steam API key:", e);
      addToast(t("settings.apiKeyClearFailed"), { type: "error" });
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
      // Only overwrite when a real path came back. A cancel now rejects with
      // AppError::Cancelled (caught below), but stay defensive against an empty
      // string too: clearing the configured path is reserved for an explicit
      // reset action, never for a dismissed picker.
      const trimmed = selected?.trim();
      if (trimmed) {
        platformPaths[platformId] = trimmed;
      }
    } catch {
      // User canceled the picker or the native dialog failed: leave the
      // existing path untouched.
    }
  }

  async function openSteamApiKeyPage() {
    try {
      await openSteamApiKeyPageInBrowser();
    } catch {
      addToast(t("settings.openApiKeyFailed"));
    }
  }

  const WIKI_URL = "https://github.com/klNuno/accshift/wiki";

  async function openHelp() {
    try {
      await invoke("open_url", { url: WIKI_URL });
    } catch {
      addToast(t("settings.openHelpFailed"), { type: "error" });
    }
  }

  async function openLogs() {
    try {
      await invoke("open_logs_folder");
    } catch {
      addToast(t("settings.openLogsFailed"), { type: "error" });
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

  async function closePanel() {
    await hydrationReady;
    await flushSettingsNow();
    await onClose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") void closePanel();
  }

  onMount(async () => {
    registerFlush(async () => {
      await hydrationReady;
      await flushSettingsNow();
    });

    try {
      normalizeSettings();
      refreshNumericInputsFromSettings();
      const settingsAtHydrationStart = JSON.parse(JSON.stringify(settings)) as AppSettings;
      const enabledIds = [...settings.enabledPlatforms];
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

      // Baseline the settings that were actually loaded, not edits made while
      // async API/path hydration was in flight. Those edits remain dirty.
      lastPersistedSnapshot = buildPersistSnapshot(settingsAtHydrationStart);
      lastPlatformSnapshot = JSON.stringify({
        enabledPlatforms: [...settingsAtHydrationStart.enabledPlatforms].sort(),
        defaultPlatformId: settingsAtHydrationStart.defaultPlatformId,
      });
      hydrated = true;

      registerSearchFocus(() => void focusPlatformSearch());
    } finally {
      resolveHydrationReady();
    }
  });

  onDestroy(() => {
    // Keep the final promise registered after unmount. A native close or
    // updater relaunch can then still await a browser-history-driven close.
    const finalPersist = hydrationReady.then(flushSettingsNow);
    registerFlush(() => finalPersist);
    registerSearchFocus(null);
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
      settings.accountDisplay.cardColorOutlines,
      showLastLoginKey,
      healthCheckKey,
      settings.uiScalePercent,
      settings.defaultPlatformId,
      settings.pinEnabled,
      settings.pinHash,
      pinCodeInput,
      settings.personasEnabled,
      settings.deepLinksEnabled,
      settings.cliEnabled,
      settings.streamerMode,
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

<svelte:window onkeydown={handleKeydown} />

<div class="settings-panel">
  <aside class="settings-sidebar">
    <span class="sidebar-title">{t("settings.title")}</span>

    <div class="nav-list" bind:this={tabBar.tabsRef}>
      {#each visibleCoreTabs as tab (tab.id)}
        <button
          class="nav-item"
          class:active={tabBar.activeTab === tab.id}
          type="button"
          data-settings-tab={tab.id}
          aria-label={t(tab.labelKey)}
          aria-current={tabBar.activeTab === tab.id ? "page" : undefined}
          title={t(tab.labelKey)}
          onclick={() => selectTab(tab.id)}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d={CORE_TAB_ICONS[tab.id] ?? CORE_TAB_ICONS.general} />
          </svg>
          <span class="nav-label">{t(tab.labelKey)}</span>
        </button>
      {/each}

      {#if visiblePlatformTabs.length > 0}
        <div class="nav-group-label">{t("settings.platforms")}</div>
        {#each visiblePlatformTabs as tab (tab.id)}
          <button
            class="nav-item"
            class:active={tabBar.activeTab === tab.id}
            type="button"
            data-settings-tab={tab.id}
            aria-label={t(tab.labelKey)}
            aria-current={tabBar.activeTab === tab.id ? "page" : undefined}
            title={t(tab.labelKey)}
            style={`--nav-accent:${tab.accent};`}
            onclick={() => selectTab(tab.id)}
          >
            <span class="nav-dot" aria-hidden="true"></span>
            <span class="nav-label">{t(tab.labelKey)}</span>
          </button>
        {/each}
      {/if}
    </div>

    <div class="sidebar-footer">
      <button
        class="help-btn"
        type="button"
        onclick={openHelp}
        title={t("settings.help")}
        aria-label={t("settings.help")}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="10" />
          <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
          <line x1="12" y1="17" x2="12.01" y2="17" />
        </svg>
        <span class="nav-label">{t("settings.helpShort")}</span>
      </button>

      <button
        class="help-btn"
        type="button"
        onclick={openLogs}
        title={t("settings.openLogs")}
        aria-label={t("settings.openLogs")}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
        </svg>
        <span class="nav-label">{t("settings.logs")}</span>
      </button>
    </div>
  </aside>

  <div class="settings-main">
    <div class="main-header">
      <span class="section-title">{t(activeTabLabelKey)}</span>
      <button
        class="close-btn"
        onclick={() => void closePanel()}
        title={t("common.close")}
        aria-label={t("common.close")}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    </div>

    <div class="settings-content" bind:this={contentRef}>
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
        registerSearchInput={(node) => (platformSearchInput = node)}
      />
    {/if}

    {#if tabBar.activeTab === "privacy"}
      <SettingsPrivacyTab
        bind:settings
        bind:pinCodeInput
        bind:pinSetupPending
        {t}
        {inactivityBlur}
        neutralAccent={NEUTRAL_CONTROL_ACCENT}
        onReplayOnboarding={async () => {
          await closePanel();
          onReplayOnboarding();
        }}
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
            {apiKeyConfigured}
            {apiKeyError}
            onClearApiKey={clearApiKey}
            pathError={platformPathErrors[platformId] ?? false}
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
</div>

<style>
  .settings-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: row;
    gap: 14px;
    overflow: hidden;
    animation: page-entrance var(--motion-page-entrance) ease-out;
  }

  :global(html[data-motion="reduced"]) .settings-panel {
    animation: none;
  }

  .settings-sidebar {
    flex: 0 0 168px;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-right: 12px;
    border-right: 1px solid color-mix(in srgb, var(--border) 75%, transparent);
  }

  .sidebar-title {
    font-size: 14px;
    font-weight: 700;
    color: var(--fg);
    padding: 4px 10px 10px;
  }

  .nav-list {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
    scrollbar-width: none;
  }

  .nav-list::-webkit-scrollbar {
    display: none;
  }

  .nav-group-label {
    margin-top: 12px;
    padding: 0 10px 4px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--fg-subtle);
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--fg-muted);
    font-size: 12px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    transition: background 120ms ease-out, color 120ms ease-out;
  }

  .nav-item svg {
    flex: 0 0 auto;
    opacity: 0.75;
  }

  .nav-item:hover {
    background: color-mix(in srgb, var(--bg-card) 82%, #fff 18%);
    color: var(--fg);
  }

  .nav-item.active {
    background: color-mix(in srgb, var(--bg-card) 74%, #fff 26%);
    color: var(--fg);
  }

  .nav-item.active svg {
    opacity: 1;
  }

  .nav-dot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    margin: 3px;
    border-radius: 999px;
    background: var(--nav-accent, var(--fg-subtle));
    opacity: 0.7;
  }

  .nav-item.active .nav-dot,
  .nav-item:hover .nav-dot {
    opacity: 1;
  }

  .nav-label {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sidebar-footer {
    display: flex;
    gap: 4px;
  }

  .help-btn {
    display: flex;
    align-items: center;
    gap: 9px;
    flex: 1;
    min-width: 0;
    padding: 8px 10px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--fg-subtle);
    font-size: 12px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    transition: background 120ms ease-out, color 120ms ease-out;
  }

  .help-btn:hover {
    background: color-mix(in srgb, var(--bg-card) 82%, #fff 18%);
    color: var(--fg);
  }

  .settings-main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .main-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-bottom: 8px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 75%, transparent);
  }

  .section-title {
    font-size: 14px;
    font-weight: 700;
    color: var(--fg);
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
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    align-self: start;
  }

  .settings-panel :global(.card-wide) {
    grid-column: span 2;
  }

  .settings-panel :global(.card h3) {
    margin: 0;
    padding-bottom: 8px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 65%, transparent);
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-muted);
  }

  /* When a header sits inside a title row (Platforms tab: h3 + search pill),
     the row owns the divider. The h3's own border must yield deterministically
     — same-specificity scoped overrides lose to the rule above at random
     depending on Svelte's per-component <style> injection order, which showed
     up as an intermittent double line. This higher-specificity, same-sheet
     rule wins every time. */
  .settings-panel :global(.card-title-row h3) {
    padding-bottom: 0;
    border-bottom: none;
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
    background: var(--bg-input);
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

    /* Icon-only sidebar on narrow windows: labels and group headers collapse. */
    .settings-sidebar {
      flex-basis: 40px;
      padding-right: 8px;
    }

    .sidebar-title,
    .nav-label,
    .nav-group-label {
      display: none;
    }

    .nav-item,
    .help-btn {
      justify-content: center;
      padding: 8px 6px;
    }

    /* Two icon-only buttons will not fit side by side in a 40px sidebar. */
    .sidebar-footer {
      flex-direction: column;
    }
  }
</style>
