<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getSettings, saveSettings, ALL_PLATFORMS } from "./store";
  import { addToast } from "../notifications/store.svelte";
  import ToggleSetting from "./ToggleSetting.svelte";
  import SteamSettingsSection from "$lib/platforms/steam/SteamSettingsSection.svelte";
  import { getPlatformDefinition } from "$lib/platforms/registry";
  import {
    DEFAULT_LOCALE,
    LANGUAGE_OPTIONS,
    normalizeLocale,
    translate,
    type MessageKey,
    type TranslationParams,
  } from "$lib/i18n";
  import { hashPinCode, sanitizePinDigits } from "$lib/shared/pin";

  type SettingsTabId = "general" | "platforms" | "privacy" | "steam" | "riot" | "battleNet";
  type SettingsTabDef = {
    id: SettingsTabId;
    labelKey: MessageKey;
    accent: string;
    visible?: () => boolean;
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
  let steamEnabled = $derived(settings.enabledPlatforms.includes("steam"));
  let riotEnabled = $derived(settings.enabledPlatforms.includes("riot"));
  let battleNetEnabled = $derived(settings.enabledPlatforms.includes("battle-net"));
  let apiKey = $state("");
  let apiKeyConfigured = $state(false);
  let apiKeyTouched = $state(false);
  let steamPath = $state("");
  let riotPath = $state("");
  let battleNetPath = $state("");
  let pinCodeInput = $state("");
  let uiScalePercentInput = $state("");
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
  let activeSettingsTab = $state<SettingsTabId>("general");
  let tabsRef = $state<HTMLDivElement | null>(null);
  let tabUiFrame: number | null = null;
  let tabResizeObserver: ResizeObserver | null = null;
  let tabsOverflowing = $state(false);
  let canScrollTabsLeft = $state(false);
  let canScrollTabsRight = $state(false);
  const SAVE_TOAST_COOLDOWN_MS = 1500;
  const PIN_CODE_LENGTH = 4;
  const NEUTRAL_TAB_ACCENT = "#71717a";
  const STEAM_TAB_ACCENT = getPlatformDefinition("steam")?.accent ?? NEUTRAL_TAB_ACCENT;
  const RIOT_TAB_ACCENT = getPlatformDefinition("riot")?.accent ?? "#ef4444";
  const RIOT_DISPLAY_ACCENT = getPlatformDefinition("riot")?.accent ?? "#ef4444";
  const BATTLE_NET_TAB_ACCENT = getPlatformDefinition("battle-net")?.accent ?? "#60a5fa";
  const languageLabelByCode: Record<string, MessageKey> = {
    en: "language.english",
    fr: "language.french",
  };

  const tabConfig: SettingsTabDef[] = [
    { id: "general", labelKey: "settings.general", accent: NEUTRAL_TAB_ACCENT },
    { id: "platforms", labelKey: "settings.platforms", accent: NEUTRAL_TAB_ACCENT },
    { id: "privacy", labelKey: "settings.privacy", accent: NEUTRAL_TAB_ACCENT },
    { id: "steam", labelKey: "settings.steam", accent: STEAM_TAB_ACCENT, visible: () => steamEnabled },
    { id: "riot", labelKey: "settings.riot", accent: RIOT_TAB_ACCENT, visible: () => riotEnabled },
    { id: "battleNet", labelKey: "settings.battleNet", accent: BATTLE_NET_TAB_ACCENT, visible: () => battleNetEnabled },
  ];

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
    if (settings.theme !== "light" && settings.theme !== "dark") {
      settings.theme = "dark";
    }
    settings.language = normalizeLocale(settings.language);
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
    return JSON.stringify({
      settings,
      pendingApiKey,
      apiKeyConfigured,
      steamPath: steamPath.trim(),
      riotPath: riotPath.trim(),
      battleNetPath: battleNetPath.trim(),
    });
  }

  function refreshNumericInputsFromSettings() {
    uiScalePercentInput = String(settings.uiScalePercent);
    avatarCacheDaysInput = String(settings.dataRefresh.avatarCacheDays);
    banCheckDaysInput = String(settings.dataRefresh.banCheckDays);
    inactivityBlurSecondsInput = String(settings.inactivityBlurSeconds);
  }

  function commitUiScalePercent() {
    settings.uiScalePercent = clampInt(Number(uiScalePercentInput), 75, 150, settings.uiScalePercent);
    uiScalePercentInput = String(settings.uiScalePercent);
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
    const nextSteamPath = steamPath.trim();
    const nextRiotPath = riotPath.trim();
    const nextBattleNetPath = battleNetPath.trim();
    const previousState = lastPersistedSnapshot
      ? JSON.parse(lastPersistedSnapshot) as {
        steamPath?: string;
        riotPath?: string;
        battleNetPath?: string;
      }
      : {};
    const steamPathChanged = (previousState.steamPath ?? "") !== nextSteamPath;
    const riotPathChanged = (previousState.riotPath ?? "") !== nextRiotPath;
    const battleNetPathChanged = (previousState.battleNetPath ?? "") !== nextBattleNetPath;

    saveSettings(settings);
    onSettingsUpdated?.();

    try {
      if (apiKeyTouched) {
        const trimmedApiKey = apiKey.trim();
        await invoke("set_api_key", { key: trimmedApiKey });
        apiKeyConfigured = trimmedApiKey.length > 0;
        apiKeyTouched = false;
        apiKey = "";
      }
      if (steamPathChanged) {
        await invoke("set_steam_path", { path: nextSteamPath });
      }
      if (riotPathChanged) {
        await invoke("set_riot_path", { path: nextRiotPath });
      }
      if (battleNetPathChanged) {
        await invoke("set_battle_net_path", { path: nextBattleNetPath });
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

  function selectSettingsTab(tabId: SettingsTabId) {
    activeSettingsTab = tabId;
    queueTabUiRefresh(true);
  }

  function scrollTabs(direction: -1 | 1) {
    const el = tabsRef;
    if (!el) return;
    el.scrollBy({
      left: Math.max(180, el.clientWidth * 0.6) * direction,
      behavior: "smooth",
    });
  }

  async function chooseSteamFolder() {
    try {
      steamPath = await invoke<string>("select_steam_path");
    } catch {
      // User canceled the picker or the native dialog failed.
    }
  }

  async function chooseRiotPath() {
    try {
      riotPath = await invoke<string>("select_riot_path");
    } catch {
      // User canceled the picker or the native dialog failed.
    }
  }

  async function chooseBattleNetPath() {
    try {
      battleNetPath = await invoke<string>("select_battle_net_path");
    } catch {
      // User canceled the picker or the native dialog failed.
    }
  }

  async function openSteamApiKeyPage() {
    try {
      await invoke("open_steam_api_key_page");
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
    const [apiKeyResult, steamPathResult, riotPathResult, battleNetPathResult] = await Promise.allSettled([
      invoke<boolean>("has_api_key"),
      invoke<string>("get_steam_path"),
      invoke<string>("get_riot_path"),
      invoke<string>("get_battle_net_path"),
    ]);

    apiKeyConfigured = apiKeyResult.status === "fulfilled" ? apiKeyResult.value : false;
    apiKey = "";
    apiKeyTouched = false;
    steamPath = steamPathResult.status === "fulfilled" ? steamPathResult.value : "";
    riotPath = riotPathResult.status === "fulfilled" ? riotPathResult.value : "";
    battleNetPath = battleNetPathResult.status === "fulfilled" ? battleNetPathResult.value : "";

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
    settings.theme;
    settings.suspendGraphicsWhenMinimized;
    settings.minimizeOnAccountSwitch;
    settings.language;
    settings.platformSettings.steam.runAsAdmin;
    settings.platformSettings.steam.launchOptions;
    settings.accountDisplay.showUsernames;
    settings.accountDisplay.showLastLogin;
    settings.accountDisplay.showCardNotesInline;
    settings.accountDisplay.showRiotLastLogin;
    settings.accountDisplay.showBattleNetLastLogin;
    settings.uiScalePercent;
    settings.defaultPlatformId;
    settings.pinEnabled;
    settings.pinHash;
    pinCodeInput;
    settings.enabledPlatforms.join(",");
    apiKey;
    apiKeyConfigured;
    apiKeyTouched;
    steamPath;
    riotPath;
    battleNetPath;
    queueSave();
  });

  $effect(() => {
    const visibleIds = visibleTabs.map((tab) => tab.id);
    if (!visibleIds.includes(activeSettingsTab)) {
      activeSettingsTab = visibleIds[0] ?? "general";
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
          <div class="field">
            <div class="row">
              <span>{t("settings.language")}</span>
              <strong>{t(languageLabelByCode[settings.language] ?? "language.english")}</strong>
            </div>
            <select class="text-input select-input" bind:value={settings.language}>
              {#each LANGUAGE_OPTIONS as option}
                <option value={option.code}>{t(option.labelKey)}</option>
              {/each}
            </select>
          </div>

          <label class="field">
            <div class="row">
              <span>{t("settings.uiZoom")}</span>
              <strong>{settings.uiScalePercent}%</strong>
            </div>
            <input
              type="number"
              min="75"
              max="150"
              step="5"
              value={uiScalePercentInput}
              oninput={(e) => uiScalePercentInput = (e.currentTarget as HTMLInputElement).value}
              onblur={commitUiScalePercent}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  commitUiScalePercent();
                  (e.currentTarget as HTMLInputElement).blur();
                }
              }}
              class="text-input number-input"
            />
          </label>

          <ToggleSetting
            label={t("settings.lightMode")}
            enabled={settings.theme === "light"}
            accent="#f59e0b"
            onLabel={t("common.on")}
            offLabel={t("common.off")}
            onToggle={() => settings.theme = settings.theme === "light" ? "dark" : "light"}
          />
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
            accent="#10b981"
            onLabel={t("common.enabled")}
            offLabel={t("common.disabled")}
            onToggle={() => settings.suspendGraphicsWhenMinimized = !settings.suspendGraphicsWhenMinimized}
          />
          <ToggleSetting
            label={t("settings.minimizeOnAccountSwitch")}
            enabled={settings.minimizeOnAccountSwitch}
            accent="#0ea5e9"
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
          <div class="field">
            <div class="row">
              <span>{t("settings.defaultOnStartup")}</span>
              <strong>{visiblePlatformOptions.find((platform) => platform.id === settings.defaultPlatformId)?.name || settings.defaultPlatformId}</strong>
            </div>
            <select class="text-input select-input" bind:value={settings.defaultPlatformId}>
              {#each visiblePlatformOptions as platform}
                {@const disabled = !settings.enabledPlatforms.includes(platform.id) || !isPlatformSelectable(platform.id)}
                <option value={platform.id} {disabled}>
                  {platform.name}{disabled ? ` ${t("settings.platformDisabledSuffix")}` : ""}
                </option>
              {/each}
            </select>
          </div>
        </section>
      </div>
    {/if}

    {#if activeSettingsTab === "privacy"}
      <div class="settings-grid">
        <section class="card">
          <h3>{t("settings.privacy")}</h3>
          <label class="field">
            <div class="row">
              <span>{t("settings.inactivityTimeout")}</span>
              <span class="hint">{t("settings.zeroDisabled")}</span>
            </div>
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
            accent="#eab308"
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
              <div class="row">
                <span>{t("settings.pinCode")}</span>
                <strong>{settings.pinHash ? t("common.configured") : t("common.missing")}</strong>
              </div>
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

    {#if activeSettingsTab === "steam" && steamEnabled}
      <div class="settings-grid">
        <section class="card platform-display-card" style={`--display-accent:${STEAM_TAB_ACCENT};`}>
          <h3>{t("settings.accountDisplay")}</h3>
          <ToggleSetting
            label={t("settings.showUsernames")}
            enabled={settings.accountDisplay.showUsernames}
            accent={STEAM_TAB_ACCENT}
            onLabel={t("common.visible")}
            offLabel={t("common.hidden")}
            onToggle={() => settings.accountDisplay.showUsernames = !settings.accountDisplay.showUsernames}
          />
          <ToggleSetting
            label={t("settings.showSteamLastLogin")}
            enabled={settings.accountDisplay.showLastLogin}
            accent={STEAM_TAB_ACCENT}
            onLabel={t("common.on")}
            offLabel={t("common.off")}
            onToggle={() => settings.accountDisplay.showLastLogin = !settings.accountDisplay.showLastLogin}
          />
        </section>

        <SteamSettingsSection
          {settings}
          bind:steamPath
          bind:apiKey
          {apiKeyConfigured}
          {avatarCacheDaysInput}
          {banCheckDaysInput}
          {avatarRefreshLoading}
          {banRefreshLoading}
          onChooseSteamFolder={chooseSteamFolder}
          onOpenSteamApiKeyPage={openSteamApiKeyPage}
          onApiKeyInput={(value) => {
            apiKey = value;
            apiKeyTouched = true;
          }}
          onAvatarCacheDaysInput={(value) => avatarCacheDaysInput = value}
          onBanCheckDaysInput={(value) => banCheckDaysInput = value}
          onCommitAvatarCacheDays={commitAvatarCacheDays}
          onCommitBanCheckDays={commitBanCheckDays}
          onRefreshAvatarsNow={handleRefreshAvatarsNow}
          onRefreshBansNow={handleRefreshBansNow}
          {t}
        />
      </div>
    {/if}

    {#if activeSettingsTab === "riot" && riotEnabled}
      <div class="settings-grid">
        <section class="card platform-display-card" style={`--display-accent:${RIOT_DISPLAY_ACCENT};`}>
          <h3>{t("settings.accountDisplay")}</h3>
          <ToggleSetting
            label={t("settings.showRiotLastLogin")}
            enabled={settings.accountDisplay.showRiotLastLogin}
            accent={RIOT_DISPLAY_ACCENT}
            onLabel={t("common.on")}
            offLabel={t("common.off")}
            onToggle={() => settings.accountDisplay.showRiotLastLogin = !settings.accountDisplay.showRiotLastLogin}
          />
        </section>

        <section class="card">
          <h3>{t("settings.riot")}</h3>
          <div class="field">
            <div class="row">
              <span>{t("settings.riotClientPath")}</span>
              <strong>{riotPath ? t("common.custom") : t("settings.autoDetected")}</strong>
            </div>
            <div class="input-row">
              <input
                id="riot-client-path"
                type="text"
                bind:value={riotPath}
                class="text-input"
                placeholder="C:\Riot Games\Riot Client\RiotClientServices.exe"
              />
              <button class="browse-btn" type="button" onclick={chooseRiotPath}>{t("common.choose")}</button>
            </div>
          </div>
        </section>
      </div>
    {/if}

    {#if activeSettingsTab === "battleNet" && battleNetEnabled}
      <div class="settings-grid">
        <section class="card platform-display-card" style={`--display-accent:${BATTLE_NET_TAB_ACCENT};`}>
          <h3>{t("settings.accountDisplay")}</h3>
          <ToggleSetting
            label={t("settings.showBattleNetLastLogin")}
            enabled={settings.accountDisplay.showBattleNetLastLogin}
            accent={BATTLE_NET_TAB_ACCENT}
            onLabel={t("common.on")}
            offLabel={t("common.off")}
            onToggle={() => settings.accountDisplay.showBattleNetLastLogin = !settings.accountDisplay.showBattleNetLastLogin}
          />
        </section>

        <section class="card">
          <h3>{t("settings.battleNet")}</h3>
          <div class="field">
            <div class="row">
              <span>{t("settings.battleNetPath")}</span>
              <strong>{battleNetPath ? t("common.custom") : t("settings.autoDetected")}</strong>
            </div>
            <div class="input-row">
              <input
                id="battle-net-path"
                type="text"
                bind:value={battleNetPath}
                class="text-input"
                placeholder="C:\Program Files (x86)\Battle.net\Battle.net Launcher.exe"
              />
              <button class="browse-btn" type="button" onclick={chooseBattleNetPath}>{t("common.choose")}</button>
            </div>
          </div>
        </section>
      </div>
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
    padding-right: 4px;
    padding-bottom: 8px;
  }

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 12px;
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

  .platform-display-card {
    border-color: color-mix(in srgb, var(--display-accent) 32%, var(--border));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--display-accent) 12%, transparent);
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
    background: color-mix(in srgb, var(--bg) 88%, #fff 12%);
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
    background: color-mix(in srgb, var(--bg) 88%, #fff 12%);
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

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .row strong {
    font-size: 12px;
    color: var(--fg);
    font-weight: 600;
  }

  .hint {
    font-size: 11px;
    color: var(--fg-subtle);
  }

  .text-input {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--fg);
    font-size: 12px;
    padding: 9px 10px;
    outline: none;
  }

  .text-input:focus {
    border-color: #3b82f6;
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

  .input-row {
    display: flex;
    gap: 8px;
  }

  .browse-btn {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-card);
    color: var(--fg);
    font-size: 12px;
    padding: 0 12px;
    cursor: pointer;
    white-space: nowrap;
  }

  .browse-btn:hover {
    background: var(--bg-card-hover);
  }

  @media (max-width: 980px) {
    .card-wide {
      grid-column: span 1;
    }
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
