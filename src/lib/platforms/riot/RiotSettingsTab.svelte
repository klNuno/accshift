<script lang="ts">
  import PlatformPathSettingsTab from "$lib/features/settings/PlatformPathSettingsTab.svelte";
  import type { AppSettings } from "$lib/features/settings/types";
  import type { MessageKey, TranslationParams } from "$lib/i18n";

  let {
    settings = $bindable(),
    path = $bindable(),
    accent,
    t,
    onChoosePath,
    pathLabelKey = "settings.riotClientPath",
    pathPlaceholder = "C:\\Riot Games\\Riot Client\\RiotClientServices.exe",
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

<PlatformPathSettingsTab
  bind:settings
  bind:path
  {accent}
  {t}
  {onChoosePath}
  platformId="riot"
  cardTitleKey="settings.riot"
  showLastLoginLabelKey="settings.showRiotLastLogin"
  {pathLabelKey}
  {pathPlaceholder}
/>
