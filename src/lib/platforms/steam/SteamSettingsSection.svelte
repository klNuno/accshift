<script lang="ts">
  import ToggleSetting from "$lib/features/settings/ToggleSetting.svelte";
  import type { AppSettings } from "$lib/features/settings/types";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    settings,
    steamPath = $bindable(),
    apiKey = $bindable(),
    avatarCacheDaysInput = "",
    banCheckDaysInput = "",
    avatarRefreshLoading = false,
    banRefreshLoading = false,
    onChooseSteamFolder,
    onOpenSteamApiKeyPage,
    onApiKeyInput = () => {},
    onAvatarCacheDaysInput = () => {},
    onBanCheckDaysInput = () => {},
    onCommitAvatarCacheDays = () => {},
    onCommitBanCheckDays = () => {},
    onRefreshAvatarsNow = async () => {},
    onRefreshBansNow = async () => {},
    accent = "#2563eb",
    t,
  }: {
    settings: AppSettings;
    steamPath: string;
    apiKey: string;
    avatarCacheDaysInput?: string;
    banCheckDaysInput?: string;
    avatarRefreshLoading?: boolean;
    banRefreshLoading?: boolean;
    onChooseSteamFolder: () => void | Promise<void>;
    onOpenSteamApiKeyPage: () => void | Promise<void>;
    onApiKeyInput?: (value: string) => void;
    onAvatarCacheDaysInput?: (value: string) => void;
    onBanCheckDaysInput?: (value: string) => void;
    onCommitAvatarCacheDays?: () => void;
    onCommitBanCheckDays?: () => void;
    onRefreshAvatarsNow?: () => void | Promise<void>;
    onRefreshBansNow?: () => void | Promise<void>;
    accent?: string;
    t: (key: MessageKey, params?: TranslationParams) => string;
  } = $props();
</script>

<section class="card steam-card platform-display-card" style={`--display-accent:${accent};`}>
  <h3>{t("settings.steam")}</h3>

  <ToggleSetting
    label={t("settings.runSteamAsAdmin")}
    enabled={settings.platformSettings.steam.runAsAdmin}
    onLabel={t("common.enabled")}
    offLabel={t("common.disabled")}
    onToggle={() => settings.platformSettings.steam.runAsAdmin = !settings.platformSettings.steam.runAsAdmin}
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

  <div class="field">
    <span class="field-label">{t("settings.steamFolder")}</span>
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
    <div class="field-label-row">
      <span class="field-label">{t("settings.steamWebApiKey")}</span>
      <button class="inline-link-btn" type="button" onclick={onOpenSteamApiKeyPage}>{t("settings.getKey")}</button>
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

  <div class="field">
    <span class="field-label">{t("settings.avatarRefresh")} <span class="hint">({t("settings.zeroEachLaunch")})</span></span>
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
    <span class="field-label">{t("settings.banCheckDelay")} <span class="hint">({t("settings.zeroEachLaunch")})</span></span>
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
