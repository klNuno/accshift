<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getSettings, saveSettings, ALL_PLATFORMS } from "./store";
  import { addToast } from "../notifications/store.svelte";
  import ToggleSetting from "./ToggleSetting.svelte";
  import {
    DEFAULT_LOCALE,
    LANGUAGE_OPTIONS,
    normalizeLocale,
    translate,
    type MessageKey,
    type TranslationParams,
  } from "$lib/i18n";

  let { onClose, onPlatformsChanged }: {
    onClose: () => void;
    onPlatformsChanged?: () => void;
  } = $props();

  let settings = $state(getSettings());
  let steamEnabled = $derived(settings.enabledPlatforms.includes("steam"));
  let apiKey = $state("");
  let steamPath = $state("");
  let uiScalePercentInput = $state("");
  let avatarCacheDaysInput = $state("");
  let banCheckDaysInput = $state("");
  let inactivityBlurSecondsInput = $state("");
  let hydrated = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let lastSavedToastAt = 0;
  let lastPersistedSnapshot = "";
  const SAVE_TOAST_COOLDOWN_MS = 1500;
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
    settings.avatarCacheDays = clampInt(settings.avatarCacheDays, 0, 90, 7);
    settings.banCheckDays = clampInt(settings.banCheckDays, 0, 90, 7);
    settings.inactivityBlurSeconds = clampInt(settings.inactivityBlurSeconds, 0, 3600, 60);
    settings.steamLaunchOptions = (settings.steamLaunchOptions || "").trim();
    settings.pinCode = (settings.pinCode || "").trim();
    if (!settings.pinEnabled) {
      settings.pinCode = "";
    }
    if (!ALL_PLATFORMS.some(p => p.id === settings.defaultPlatformId)) {
      settings.defaultPlatformId = "steam";
    }
    if (!settings.enabledPlatforms.length) settings.enabledPlatforms = ["steam"];
    if (!settings.enabledPlatforms.includes(settings.defaultPlatformId)) {
      settings.defaultPlatformId = settings.enabledPlatforms[0];
    }
  }

  function buildPersistSnapshot(): string {
    return JSON.stringify({
      settings,
      apiKey: apiKey.trim(),
      steamPath: steamPath.trim(),
    });
  }

  function refreshNumericInputsFromSettings() {
    uiScalePercentInput = String(settings.uiScalePercent);
    avatarCacheDaysInput = String(settings.avatarCacheDays);
    banCheckDaysInput = String(settings.banCheckDays);
    inactivityBlurSecondsInput = String(settings.inactivityBlurSeconds);
  }

  function commitUiScalePercent() {
    settings.uiScalePercent = clampInt(Number(uiScalePercentInput), 75, 150, settings.uiScalePercent);
    uiScalePercentInput = String(settings.uiScalePercent);
  }

  function commitAvatarCacheDays() {
    settings.avatarCacheDays = clampInt(Number(avatarCacheDaysInput), 0, 90, settings.avatarCacheDays);
    avatarCacheDaysInput = String(settings.avatarCacheDays);
  }

  function commitBanCheckDays() {
    settings.banCheckDays = clampInt(Number(banCheckDaysInput), 0, 90, settings.banCheckDays);
    banCheckDaysInput = String(settings.banCheckDays);
  }

  function commitInactivityBlurSeconds() {
    settings.inactivityBlurSeconds = clampInt(Number(inactivityBlurSecondsInput), 0, 3600, settings.inactivityBlurSeconds);
    inactivityBlurSecondsInput = String(settings.inactivityBlurSeconds);
  }

  async function persistNow() {
    normalizeSettings();
    const snapshot = buildPersistSnapshot();
    if (snapshot === lastPersistedSnapshot) return;

    saveSettings(settings);
    try {
      await invoke("set_api_key", { key: apiKey.trim() });
      await invoke("set_steam_path", { path: steamPath.trim() });
      lastPersistedSnapshot = snapshot;
      const now = Date.now();
      if (now - lastSavedToastAt >= SAVE_TOAST_COOLDOWN_MS) {
        addToast(t("settings.saved"));
        lastSavedToastAt = now;
      }
      onPlatformsChanged?.();
    } catch (e) {
      console.error("Failed to save API key:", e);
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
      apiKey = await invoke<string>("get_api_key");
    } catch {
      apiKey = "";
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

  function togglePlatform(id: string) {
    if (settings.enabledPlatforms.includes(id)) {
      if (settings.enabledPlatforms.length <= 1) return;
      settings.enabledPlatforms = settings.enabledPlatforms.filter(p => p !== id);
    } else {
      settings.enabledPlatforms = [...settings.enabledPlatforms, id];
    }
  }

  $effect(() => {
    settings.avatarCacheDays;
    settings.banCheckDays;
    settings.inactivityBlurSeconds;
    settings.theme;
    settings.language;
    settings.steamRunAsAdmin;
    settings.steamLaunchOptions;
    settings.showUsernames;
    settings.showLastLogin;
    settings.showCardNotesInline;
    settings.uiScalePercent;
    settings.defaultPlatformId;
    settings.pinEnabled;
    settings.pinCode;
    settings.enabledPlatforms.join(",");
    apiKey;
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
          <button class="platform-chip" onclick={() => togglePlatform(platform.id)} style={`--chip-accent:${platform.accent};`}>
            <span>{platform.name}</span>
            <div class="toggle" class:active={settings.enabledPlatforms.includes(platform.id)}>
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
            <option value={platform.id} disabled={!settings.enabledPlatforms.includes(platform.id)}>
              {platform.name}{!settings.enabledPlatforms.includes(platform.id) ? ` ${t("settings.platformDisabledSuffix")}` : ""}
            </option>
          {/each}
        </select>
      </div>
    </section>

    <section class="card">
      <h3>{t("settings.dataRefresh")}</h3>

      <label class="field">
        <div class="row">
          <span>{t("settings.avatarRefresh")}</span>
          <span class="hint">{t("settings.zeroEachLaunch")}</span>
        </div>
        <input
          type="number"
          min="0"
          max="90"
          step="1"
          value={avatarCacheDaysInput}
          oninput={(e) => avatarCacheDaysInput = (e.currentTarget as HTMLInputElement).value}
          onblur={commitAvatarCacheDays}
          onkeydown={(e) => {
            if (e.key === "Enter") {
              commitAvatarCacheDays();
              (e.currentTarget as HTMLInputElement).blur();
            }
          }}
          class="text-input number-input"
        />
      </label>

      <label class="field">
        <div class="row">
          <span>{t("settings.banCheckDelay")}</span>
          <span class="hint">{t("settings.zeroEachLaunch")}</span>
        </div>
        <input
          type="number"
          min="0"
          max="90"
          step="1"
          value={banCheckDaysInput}
          oninput={(e) => banCheckDaysInput = (e.currentTarget as HTMLInputElement).value}
          onblur={commitBanCheckDays}
          onkeydown={(e) => {
            if (e.key === "Enter") {
              commitBanCheckDays();
              (e.currentTarget as HTMLInputElement).blur();
            }
          }}
          class="text-input number-input"
        />
      </label>
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
            settings.pinCode = "";
          }
        }}
      />

      {#if settings.pinEnabled}
        <div class="field">
          <div class="row">
            <span>{t("settings.pinCode")}</span>
            <strong>{settings.pinCode ? t("common.configured") : t("common.missing")}</strong>
          </div>
          <input
            id="pin-code"
            type="password"
            bind:value={settings.pinCode}
            class="text-input"
            placeholder={t("settings.pinPlaceholder")}
            maxlength="16"
          />
        </div>
      {/if}
    </section>

    {#if steamEnabled}
      <section class="card steam-card">
        <h3>{t("settings.steam")}</h3>

        <ToggleSetting
          label={t("settings.runSteamAsAdmin")}
          enabled={settings.steamRunAsAdmin}
          onLabel={t("common.enabled")}
          offLabel={t("common.disabled")}
          onToggle={() => settings.steamRunAsAdmin = !settings.steamRunAsAdmin}
        />

        <ToggleSetting
          label={t("settings.showUsernames")}
          enabled={settings.showUsernames}
          onLabel={t("common.visible")}
          offLabel={t("common.hidden")}
          onToggle={() => settings.showUsernames = !settings.showUsernames}
        />

        <ToggleSetting
          label={t("settings.showLastLogin")}
          enabled={settings.showLastLogin}
          onLabel={t("common.on")}
          offLabel={t("common.off")}
          onToggle={() => settings.showLastLogin = !settings.showLastLogin}
        />

        <ToggleSetting
          label={t("settings.showNotesUnderCards")}
          enabled={settings.showCardNotesInline}
          onLabel={t("common.inline")}
          offLabel={t("common.tooltip")}
          onToggle={() => settings.showCardNotesInline = !settings.showCardNotesInline}
        />

        <div class="field">
          <div class="row">
            <span>{t("settings.launchOptions")}</span>
            <strong>{settings.steamLaunchOptions ? t("common.custom") : t("common.none")}</strong>
          </div>
          <input
            id="steam-launch-options"
            type="text"
            bind:value={settings.steamLaunchOptions}
            class="text-input"
            placeholder="-silent -vgui"
          />
        </div>

        <div class="field">
          <div class="row">
            <span>{t("settings.steamFolder")}</span>
            <strong>{steamPath ? t("common.custom") : t("settings.steamFolderRegistry")}</strong>
          </div>
          <div class="input-row">
            <input
              id="steam-folder"
              type="text"
              bind:value={steamPath}
              class="text-input"
              placeholder="C:\Program Files (x86)\Steam"
            />
            <button class="browse-btn" type="button" onclick={chooseSteamFolder}>{t("common.choose")}</button>
          </div>
        </div>

        <div class="field">
          <div class="row">
            <span>{t("settings.steamWebApiKey")}</span>
            <div class="row-actions">
              <button class="inline-link-btn" type="button" onclick={openSteamApiKeyPage}>{t("settings.getKey")}</button>
              <strong>{apiKey.trim() ? t("common.configured") : t("common.missing")}</strong>
            </div>
          </div>
          <input id="api-key" type="password" bind:value={apiKey} class="text-input" placeholder={t("settings.pasteApiKey")} />
        </div>
      </section>
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

  .steam-card {
    grid-column: span 2;
  }

  @media (max-width: 980px) {
    .steam-card {
      grid-column: span 1;
    }
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

  .platform-chip:hover {
    border-color: color-mix(in srgb, var(--chip-accent) 55%, var(--border));
    background: color-mix(in srgb, var(--bg-card) 84%, #fff 16%);
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

  .row-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .inline-link-btn {
    border: none;
    background: transparent;
    color: #60a5fa;
    font-size: 12px;
    cursor: pointer;
    padding: 0;
  }

  .inline-link-btn:hover {
    color: #93c5fd;
    text-decoration: underline;
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

  .input-row {
    display: flex;
    gap: 8px;
  }

  .number-input {
    width: 100%;
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
</style>
