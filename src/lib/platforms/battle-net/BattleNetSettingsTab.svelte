<script lang="ts">
  import ToggleSetting from "$lib/features/settings/ToggleSetting.svelte";
  import type { AppSettings } from "$lib/features/settings/types";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

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

<section class="card platform-display-card" style={`--display-accent:${accent};`}>
  <h3>{t("settings.accountDisplay")}</h3>
  <ToggleSetting
    label={t("settings.showBattleNetLastLogin")}
    enabled={settings.accountDisplay.showLastLoginPerPlatform["battle-net"] ?? true}
    accent={accent}
    onLabel={t("common.on")}
    offLabel={t("common.off")}
    onToggle={() => settings.accountDisplay.showLastLoginPerPlatform["battle-net"] = !settings.accountDisplay.showLastLoginPerPlatform["battle-net"]}
  />
</section>

<section class="card platform-display-card" style={`--display-accent:${accent};`}>
  <h3>{t("settings.battleNet")}</h3>
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
</section>

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
