<script lang="ts">
  import ToggleSetting from "$lib/features/settings/ToggleSetting.svelte";
  import type { AppSettings } from "$lib/features/settings/types";
  import type { MessageKey, TranslationParams } from "$lib/i18n";
  import SettingsCard from "$lib/shared/components/SettingsCard.svelte";

  let {
    settings = $bindable(),
    path = $bindable(),
    accent,
    t,
    onChoosePath,
    pathLabelKey = "settings.battleNetPath",
    pathPlaceholder = "C:\\Program Files (x86)\\Battle.net\\Battle.net Launcher.exe",
  }: {
    settings: AppSettings;
    path: string;
    accent: string;
    t: (key: MessageKey, params?: TranslationParams) => string;
    onChoosePath: () => void | Promise<void>;
    pathLabelKey?: string;
    pathPlaceholder?: string;
  } = $props();
</script>

<SettingsCard title={t("settings.accountDisplay")} {accent}>
  <ToggleSetting
    label={t("settings.showBattleNetLastLogin")}
    enabled={settings.accountDisplay.showLastLoginPerPlatform["battle-net"] ?? false}
    accent={accent}
    onLabel={t("common.on")}
    offLabel={t("common.off")}
    onToggle={() => settings.accountDisplay.showLastLoginPerPlatform["battle-net"] = !settings.accountDisplay.showLastLoginPerPlatform["battle-net"]}
  />
</SettingsCard>

<SettingsCard title={t("settings.battleNet")} {accent}>
  <div class="field">
    <span class="field-label">{t(pathLabelKey as MessageKey)}</span>
    <div class="input-row">
      <input
        type="text"
        bind:value={path}
        class="text-input"
        placeholder={pathPlaceholder}
      />
      <button class="browse-btn" type="button" onclick={onChoosePath}>{t("common.choose")}</button>
    </div>
  </div>
</SettingsCard>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .field-label {
    font-size: 12px;
    color: var(--fg-muted);
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
