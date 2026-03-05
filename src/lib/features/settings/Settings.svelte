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

  let { onClose, onPlatformsChanged, onRefreshAvatarsNow = async () => {}, onRefreshBansNow = async () => {}, runtimeOs = "unknown", activePlatformId = "steam" }: {
    onClose: () => void;
    onPlatformsChanged?: () => void;
    onRefreshAvatarsNow?: () => void | Promise<void>;
    onRefreshBansNow?: () => void | Promise<void>;
    runtimeOs?: "windows" | "linux" | "macos" | "unknown";
    activePlatformId?: string;
  } = $props();

  let settings = $state(getSettings());
  let steamEnabled = $derived(settings.enabledPlatforms.includes("steam"));
  let steamToolsEnabled = $derived(activePlatformId === "steam");
  let apiKey = $state("");
  let apiKeyConfigured = $state(false);
  let apiKeyTouched = $state(false);
  let steamPath = $state("");
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
  const SAVE_TOAST_COOLDOWN_MS = 1500;
  const PIN_CODE_LENGTH = 4;
  const languageLabelByCode: Record<string, MessageKey> = {
    en: "language.english",
    fr: "language.french",
  };

  function t(key: MessageKey, params?: TranslationParams): string {
    return translate(settings.language ?? DEFAULT_LOCALE, key, params);
  }

  function clampInt(value: number, min: number, max: number, fallback: number): number {
    if (!Number.isFinite(value)) return fallback;
    return Math.min(max, Math.max(min, Math.round(value)));
  }

  function normalizeSettings() {
    if (settings.theme !== "light" && settings.theme !== "dark") {
      settings.theme = "dark";
    }
    settings.language = normalizeLocale(settings.language);
    settings.uiScalePercent = clampInt(settings.uiScalePercent, 75, 150, 100);
    settings.dataRefresh.avatarCacheDays = clampInt(settings.dataRefresh.avatarCacheDays, 0, 90, 7);
    settings.dataRefresh.banCheckDays = clampInt(settings.dataRefresh.banCheckDays, 0, 90, 7);
    settings.inactivityBlurSeconds = clampInt(settings.inactivityBlurSeconds, 0, 3600, 60);
    settings.platformSettings.steam.launchOptions = (settings.platformSettings.steam.launchOptions || "").trim();
    if (!settings.pinEnabled) {
      settings.pinHash = "";
    }
    if (!ALL_PLATFORMS.some(p => p.id === settings.defaultPlatformId)) {
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

    saveSettings(settings);
    try {
      if (apiKeyTouched) {
        const trimmedApiKey = apiKey.trim();
        await invoke("set_api_key", { key: trimmedApiKey });
        apiKeyConfigured = trimmedApiKey.length > 0;
        apiKeyTouched = false;
        apiKey = "";
      }
      await invoke("set_steam_path", { path: steamPath.trim() });
      lastPersistedSnapshot = buildPersistSnapshot();
      const now = Date.now();
      if (now - lastSavedToastAt >= SAVE_TOAST_COOLDOWN_MS) {
        addToast(t("settings.saved"));
        lastSavedToastAt = now;
      }
      onPlatformsChanged?.();
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

  onMount(async () => {
    try {
      apiKeyConfigured = await invoke<boolean>("has_api_key");
    } catch {
      apiKeyConfigured = false;
    } finally {
      apiKey = "";
      apiKeyTouched = false;
    }

    try {
      steamPath = await invoke<string>("get_steam_path");
    } catch {
      steamPath = "";
    } finally {
      normalizeSettings();
      refreshNumericInputsFromSettings();
      lastPersistedSnapshot = buildPersistSnapshot();
      hydrated = true;
    }
  });

  onDestroy(() => {
    if (saveTimer) clearTimeout(saveTimer);
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
      settings.enabledPlatforms = settings.enabledPlatforms.filter(p => p !== id);
    } else {
      if (!isPlatformSelectable(id)) return;
      settings.enabledPlatforms = [...settings.enabledPlatforms, id];
    }
  }

  $effect(() => {
    settings.dataRefresh.avatarCacheDays;
    settings.dataRefresh.banCheckDays;
    settings.inactivityBlurSeconds;
    settings.theme;
    settings.language;
    settings.platformSettings.steam.runAsAdmin;
    settings.platformSettings.steam.launchOptions;
    settings.accountDisplay.showUsernames;
    settings.accountDisplay.showLastLogin;
    settings.accountDisplay.showCardNotesInline;
    settings.accountDisplay.showRiotLastLogin;
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
    queueSave();
  });

  async function chooseSteamFolder() {
    try {
      steamPath = await invoke<string>("select_steam_path");
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
</script>

<svelte:window onkeydown={handleKeydown} />

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

  <div class="settings-grid">
    <section class="card">
      <h3>{t("settings.appearance")}</h3>
      <div class="field">
        <div class="row">
          <span>{t("settings.language")}</span>
          <strong>
            {t(languageLabelByCode[settings.language] ?? "language.english")}
          </strong>
        </div>
        <select class="text-input" bind:value={settings.language}>
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
      <h3>{t("settings.platforms")}</h3>
      <div class="platforms">
        {#each ALL_PLATFORMS as platform}
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

      <div class="field">
        <div class="row">
          <span>{t("settings.defaultOnStartup")}</span>
          <strong>{ALL_PLATFORMS.find(p => p.id === settings.defaultPlatformId)?.name || settings.defaultPlatformId}</strong>
        </div>
        <select class="text-input" bind:value={settings.defaultPlatformId}>
          {#each ALL_PLATFORMS as platform}
            {@const disabled = !settings.enabledPlatforms.includes(platform.id) || !isPlatformSelectable(platform.id)}
            <option value={platform.id} {disabled}>
              {platform.name}{disabled ? ` ${t("settings.platformDisabledSuffix")}` : ""}
            </option>
          {/each}
        </select>
      </div>
    </section>

    <section class="card">
      <h3>{t("settings.accountDisplay")}</h3>
      <ToggleSetting
        label={t("settings.showUsernames")}
        enabled={settings.accountDisplay.showUsernames}
        onLabel={t("common.visible")}
        offLabel={t("common.hidden")}
        onToggle={() => settings.accountDisplay.showUsernames = !settings.accountDisplay.showUsernames}
      />
      <ToggleSetting
        label={t("settings.showSteamLastLogin")}
        enabled={settings.accountDisplay.showLastLogin}
        onLabel={t("common.on")}
        offLabel={t("common.off")}
        onToggle={() => settings.accountDisplay.showLastLogin = !settings.accountDisplay.showLastLogin}
      />
      <ToggleSetting
        label={t("settings.showRiotLastLogin")}
        enabled={settings.accountDisplay.showRiotLastLogin}
        onLabel={t("common.on")}
        offLabel={t("common.off")}
        onToggle={() => settings.accountDisplay.showRiotLastLogin = !settings.accountDisplay.showRiotLastLogin}
      />
      <ToggleSetting
        label={t("settings.showNotesUnderCards")}
        enabled={settings.accountDisplay.showCardNotesInline}
        onLabel={t("common.inline")}
        offLabel={t("common.tooltip")}
        onToggle={() => settings.accountDisplay.showCardNotesInline = !settings.accountDisplay.showCardNotesInline}
      />
    </section>

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

    {#if steamEnabled}
      <SteamSettingsSection
        {settings}
        bind:steamPath
        bind:apiKey
        showSteamTools={steamToolsEnabled}
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
    {/if}
  </div>
</div>

<style>
  .settings-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
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

  .settings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 10px;
    padding-bottom: 8px;
  }

  .card {
    background: color-mix(in srgb, var(--bg-card) 84%, #000 16%);
    border: 1px solid color-mix(in srgb, var(--border) 80%, #fff 20%);
    border-radius: 10px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .card h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 650;
    color: var(--fg);
  }

  .platforms {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .platform-chip {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bg) 88%, #fff 12%);
    color: var(--fg);
    padding: 9px 10px;
    cursor: pointer;
    transition: border-color 120ms ease-out, background 120ms ease-out;
  }

  .platform-main {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
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

  .number-input {
    width: 100%;
  }

</style>
