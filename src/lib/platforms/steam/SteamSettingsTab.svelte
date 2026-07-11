<script lang="ts">
  import ToggleSetting from "$lib/features/settings/ToggleSetting.svelte";
  import SteamSettingsSection from "./SteamSettingsSection.svelte";
  import Cs2BridgeSettings from "./Cs2BridgeSettings.svelte";
  import type { AppSettings } from "$lib/features/settings/types";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    settings = $bindable(),
    path = $bindable(),
    accent,
    t,
    apiKey = $bindable(),
    apiKeyConfigured = false,
    apiKeyError = false,
    pathError = false,
    avatarCacheDaysInput = "",
    banCheckDaysInput = "",
    avatarRefreshLoading = false,
    banRefreshLoading = false,
    onChoosePath,
    onOpenSteamApiKeyPage,
    onApiKeyInput = () => {},
    onClearApiKey = () => {},
    onAvatarCacheDaysInput = () => {},
    onBanCheckDaysInput = () => {},
    onCommitAvatarCacheDays = () => {},
    onCommitBanCheckDays = () => {},
    onRefreshAvatarsNow = async () => {},
    onRefreshBansNow = async () => {},
    pathLabelKey = "settings.steamFolder",
    pathPlaceholder = "C:\\Program Files (x86)\\Steam",
  }: {
    settings: AppSettings;
    path: string;
    accent: string;
    t: (key: MessageKey, params?: TranslationParams) => string;
    apiKey: string;
    apiKeyConfigured?: boolean;
    apiKeyError?: boolean;
    pathError?: boolean;

    avatarCacheDaysInput?: string;
    banCheckDaysInput?: string;
    avatarRefreshLoading?: boolean;
    banRefreshLoading?: boolean;
    onChoosePath: () => void | Promise<void>;
    onOpenSteamApiKeyPage: () => void | Promise<void>;
    onApiKeyInput?: (value: string) => void;
    onClearApiKey?: () => void | Promise<void>;
    onAvatarCacheDaysInput?: (value: string) => void;
    onBanCheckDaysInput?: (value: string) => void;
    onCommitAvatarCacheDays?: () => void;
    onCommitBanCheckDays?: () => void;
    onRefreshAvatarsNow?: () => void | Promise<void>;
    onRefreshBansNow?: () => void | Promise<void>;
    pathLabelKey?: string;
    pathPlaceholder?: string;
  } = $props();
</script>

<section class="card platform-display-card" style={`--display-accent:${accent};`}>
  <h3>{t("settings.accountDisplay")}</h3>
  <ToggleSetting
    label={t("settings.showUsernames")}
    enabled={settings.accountDisplay.showUsernames}
    accent={accent}
    onLabel={t("common.visible")}
    offLabel={t("common.hidden")}
    onToggle={() => settings.accountDisplay.showUsernames = !settings.accountDisplay.showUsernames}
  />
  <ToggleSetting
    label={t("settings.showSteamLastLogin")}
    enabled={settings.accountDisplay.showLastLoginPerPlatform["steam"] ?? false}
    accent={accent}
    onLabel={t("common.on")}
    offLabel={t("common.off")}
    onToggle={() => settings.accountDisplay.showLastLoginPerPlatform["steam"] = !settings.accountDisplay.showLastLoginPerPlatform["steam"]}
  />
</section>

<SteamSettingsSection
  {settings}
  bind:steamPath={path}
  bind:apiKey
  {accent}
  {apiKeyConfigured}
  {apiKeyError}
  {pathError}
  {pathLabelKey}
  {pathPlaceholder}

  {avatarCacheDaysInput}
  {banCheckDaysInput}
  {avatarRefreshLoading}
  {banRefreshLoading}
  onChooseSteamFolder={onChoosePath}
  onOpenSteamApiKeyPage={onOpenSteamApiKeyPage}
  onApiKeyInput={onApiKeyInput}
  onClearApiKey={onClearApiKey}
  onAvatarCacheDaysInput={onAvatarCacheDaysInput}
  onBanCheckDaysInput={onBanCheckDaysInput}
  onCommitAvatarCacheDays={onCommitAvatarCacheDays}
  onCommitBanCheckDays={onCommitBanCheckDays}
  onRefreshAvatarsNow={onRefreshAvatarsNow}
  onRefreshBansNow={onRefreshBansNow}
  {t}
/>

<Cs2BridgeSettings {accent} {t} />

<style>
  .card {
    background: color-mix(in srgb, var(--bg-card) 84%, #000 16%);
    border: 1px solid color-mix(in srgb, var(--border) 80%, #fff 20%);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
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
</style>
