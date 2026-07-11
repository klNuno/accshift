<script lang="ts">
  import ToggleSetting from "$lib/features/settings/ToggleSetting.svelte";
  import type { AppSettings } from "$lib/features/settings/types";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    settings,
    steamPath = $bindable(),
    apiKey = $bindable(),
    apiKeyConfigured = false,
    apiKeyError = false,
    pathError = false,
    avatarCacheDaysInput = "",
    banCheckDaysInput = "",
    avatarRefreshLoading = false,
    banRefreshLoading = false,
    onChooseSteamFolder,
    onOpenSteamApiKeyPage,
    onApiKeyInput = () => {},
    onClearApiKey = () => {},
    onAvatarCacheDaysInput = () => {},
    onBanCheckDaysInput = () => {},
    onCommitAvatarCacheDays = () => {},
    onCommitBanCheckDays = () => {},
    onRefreshAvatarsNow = async () => {},
    onRefreshBansNow = async () => {},
    accent = "#2563eb",
    pathLabelKey = "settings.steamFolder",
    pathPlaceholder = "C:\\Program Files (x86)\\Steam",
    t,
  }: {
    settings: AppSettings;
    steamPath: string;
    apiKey: string;
    apiKeyConfigured?: boolean;
    apiKeyError?: boolean;
    pathError?: boolean;
    avatarCacheDaysInput?: string;
    banCheckDaysInput?: string;
    avatarRefreshLoading?: boolean;
    banRefreshLoading?: boolean;
    onChooseSteamFolder: () => void | Promise<void>;
    onOpenSteamApiKeyPage: () => void | Promise<void>;
    onApiKeyInput?: (value: string) => void;
    onClearApiKey?: () => void | Promise<void>;
    onAvatarCacheDaysInput?: (value: string) => void;
    onBanCheckDaysInput?: (value: string) => void;
    onCommitAvatarCacheDays?: () => void;
    onCommitBanCheckDays?: () => void;
    onRefreshAvatarsNow?: () => void | Promise<void>;
    onRefreshBansNow?: () => void | Promise<void>;
    accent?: string;
    pathLabelKey?: string;
    pathPlaceholder?: string;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();
</script>

<section class="card platform-display-card" style={`--display-accent:${accent};`}>
  <h3>{t("settings.steamLaunch")}</h3>

  <ToggleSetting
    label={t("settings.runSteamAsAdmin")}
    enabled={settings.platformSettings.steam.runAsAdmin}
    onLabel={t("common.enabled")}
    offLabel={t("common.disabled")}
    onToggle={() => settings.platformSettings.steam.runAsAdmin = !settings.platformSettings.steam.runAsAdmin}
  />

  <ToggleSetting
    label={t("settings.shutdownMode")}
    enabled={settings.platformSettings.steam.shutdownMode === "force"}
    onLabel={t("settings.shutdownModeForce")}
    offLabel={t("settings.shutdownModeGraceful")}
    onToggle={() => settings.platformSettings.steam.shutdownMode = settings.platformSettings.steam.shutdownMode === "force" ? "graceful" : "force"}
  />

  <label class="field">
    <span class="field-label">{t("settings.launchOptions")}</span>
    <input
      id="steam-launch-options"
      type="text"
      bind:value={settings.platformSettings.steam.launchOptions}
      class="text-input"
      placeholder="-silent -vgui"
    />
  </label>
</section>

<section class="card platform-display-card" style={`--display-accent:${accent};`}>
  <h3>{t("settings.steamInstallation")}</h3>

  <div class="field">
    <span class="field-label">{t(pathLabelKey as MessageKey)}</span>
    <div class="input-row">
      <input
        id="steam-folder"
        type="text"
        bind:value={steamPath}
        class="text-input"
        class:invalid={pathError}
        placeholder={pathPlaceholder}
      />
      <button class="browse-btn" type="button" onclick={onChooseSteamFolder}>{t("common.choose")}</button>
    </div>
    {#if pathError}
      <p class="error-hint">{t("settings.pathInvalidHint")}</p>
    {/if}
  </div>
</section>

<section class="card platform-display-card" style={`--display-accent:${accent};`}>
  <h3>{t("settings.steamApiAndData")}</h3>

  <div class="field">
    <div class="field-label-row">
      <span class="field-label">
        {t("settings.steamWebApiKey")}
        {#if apiKeyConfigured}
          <span class="key-badge">{t("common.configured")}</span>
        {/if}
      </span>
      <button class="inline-link-btn" type="button" onclick={onOpenSteamApiKeyPage}>{t("settings.getKey")}</button>
    </div>
    <div class="input-row">
      <input
        id="api-key"
        type="password"
        bind:value={apiKey}
        class="text-input"
        class:invalid={apiKeyError}
        placeholder={t("settings.pasteApiKey")}
        oninput={(e) => onApiKeyInput((e.currentTarget as HTMLInputElement).value)}
      />
      {#if apiKeyConfigured}
        <button class="clear-key-btn" type="button" onclick={onClearApiKey}>{t("settings.clearApiKey")}</button>
      {/if}
    </div>
    {#if apiKeyError}
      <p class="error-hint">{t("settings.apiKeyInvalidHint")}</p>
    {/if}
  </div>

  <div class="field">
    <span class="field-label">{t("settings.avatarRefresh")} <span class="hint">({t("settings.unitDays")}, {t("settings.zeroEachLaunch")})</span></span>
    <div class="input-row">
      <input
        type="number"
        min="0"
        max="90"
        step="1"
        value={avatarCacheDaysInput}
        oninput={(e) => onAvatarCacheDaysInput((e.currentTarget as HTMLInputElement).value)}
        onblur={onCommitAvatarCacheDays}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            onCommitAvatarCacheDays();
            (e.currentTarget as HTMLInputElement).blur();
          }
        }}
        class="text-input number-input"
      />
      <button
        class="browse-btn"
        type="button"
        onclick={onRefreshAvatarsNow}
        disabled={avatarRefreshLoading}
      >
        {t("settings.refreshNow")}
      </button>
    </div>
  </div>

  <div class="field">
    <span class="field-label">{t("settings.banCheckDelay")} <span class="hint">({t("settings.unitDays")}, {t("settings.zeroEachLaunch")})</span></span>
    <div class="input-row">
      <input
        type="number"
        min="0"
        max="90"
        step="1"
        value={banCheckDaysInput}
        oninput={(e) => onBanCheckDaysInput((e.currentTarget as HTMLInputElement).value)}
        onblur={onCommitBanCheckDays}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            onCommitBanCheckDays();
            (e.currentTarget as HTMLInputElement).blur();
          }
        }}
        class="text-input number-input"
      />
      <button
        class="browse-btn"
        type="button"
        onclick={onRefreshBansNow}
        disabled={banRefreshLoading}
      >
        {t("settings.refreshNow")}
      </button>
    </div>
  </div>
</section>

<style>
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
    padding-bottom: 8px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 65%, transparent);
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-muted);
  }

  .platform-display-card {
    border-color: color-mix(in srgb, var(--display-accent) 32%, var(--border));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--display-accent) 12%, transparent);
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

  .field-label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .hint {
    font-size: 11px;
    color: var(--fg-subtle);
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
    background: var(--bg-solid);
    color: var(--fg);
    font-size: 12px;
    padding: 9px 10px;
    outline: none;
  }

  .text-input:focus {
    border-color: color-mix(in srgb, var(--fg-muted) 55%, var(--border));
  }

  .text-input.invalid,
  .text-input.invalid:focus {
    border-color: #ef4444;
  }

  .error-hint {
    margin: 0;
    font-size: 11px;
    color: #fca5a5;
  }

  .key-badge {
    display: inline-block;
    margin-left: 6px;
    padding: 1px 7px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, #22c55e 45%, var(--border));
    background: color-mix(in srgb, #22c55e 14%, transparent);
    color: #4ade80;
    font-size: 10px;
    font-weight: 600;
    vertical-align: middle;
  }

  .clear-key-btn {
    flex-shrink: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-card);
    color: var(--fg-muted);
    font-size: 11px;
    padding: 0 10px;
    cursor: pointer;
    white-space: nowrap;
    transition: border-color 120ms ease-out, color 120ms ease-out;
  }

  .clear-key-btn:hover {
    color: #fca5a5;
    border-color: color-mix(in srgb, #ef4444 45%, var(--border));
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
