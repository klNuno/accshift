<script lang="ts">
  import ToggleSetting from "$lib/features/settings/ToggleSetting.svelte";
  import type { AppSettings } from "$lib/features/settings/types";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    settings,
    steamPath = $bindable(),
    apiKey = $bindable(),
    apiKeyConfigured = false,
    onChooseSteamFolder,
    onOpenSteamApiKeyPage,
    onApiKeyInput = () => {},
    t,
  }: {
    settings: AppSettings;
    steamPath: string;
    apiKey: string;
    apiKeyConfigured?: boolean;
    onChooseSteamFolder: () => void | Promise<void>;
    onOpenSteamApiKeyPage: () => void | Promise<void>;
    onApiKeyInput?: (value: string) => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();
</script>

<section class="card steam-card">
  <h3>{t("settings.steam")}</h3>

  <ToggleSetting
    label={t("settings.runSteamAsAdmin")}
    enabled={settings.platformSettings.steam.runAsAdmin}
    onLabel={t("common.enabled")}
    offLabel={t("common.disabled")}
    onToggle={() => settings.platformSettings.steam.runAsAdmin = !settings.platformSettings.steam.runAsAdmin}
  />

  <ToggleSetting
    label={t("settings.showUsernames")}
    enabled={settings.accountDisplay.showUsernames}
    onLabel={t("common.visible")}
    offLabel={t("common.hidden")}
    onToggle={() => settings.accountDisplay.showUsernames = !settings.accountDisplay.showUsernames}
  />

  <ToggleSetting
    label={t("settings.showLastLogin")}
    enabled={settings.accountDisplay.showLastLogin}
    onLabel={t("common.on")}
    offLabel={t("common.off")}
    onToggle={() => settings.accountDisplay.showLastLogin = !settings.accountDisplay.showLastLogin}
  />

  <ToggleSetting
    label={t("settings.showNotesUnderCards")}
    enabled={settings.accountDisplay.showCardNotesInline}
    onLabel={t("common.inline")}
    offLabel={t("common.tooltip")}
    onToggle={() => settings.accountDisplay.showCardNotesInline = !settings.accountDisplay.showCardNotesInline}
  />

  <div class="field">
    <div class="row">
      <span>{t("settings.launchOptions")}</span>
      <strong>{settings.platformSettings.steam.launchOptions ? t("common.custom") : t("common.none")}</strong>
    </div>
    <input
      id="steam-launch-options"
      type="text"
      bind:value={settings.platformSettings.steam.launchOptions}
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
      <button class="browse-btn" type="button" onclick={onChooseSteamFolder}>{t("common.choose")}</button>
    </div>
  </div>

  <div class="field">
    <div class="row">
      <span>{t("settings.steamWebApiKey")}</span>
      <div class="row-actions">
        <button class="inline-link-btn" type="button" onclick={onOpenSteamApiKeyPage}>{t("settings.getKey")}</button>
        <strong>{apiKey.trim() || apiKeyConfigured ? t("common.configured") : t("common.missing")}</strong>
      </div>
    </div>
    <input
      id="api-key"
      type="password"
      bind:value={apiKey}
      class="text-input"
      placeholder={t("settings.pasteApiKey")}
      oninput={(e) => onApiKeyInput((e.currentTarget as HTMLInputElement).value)}
    />
  </div>
</section>

<style>
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
