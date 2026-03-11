import { getSettings, ALL_PLATFORMS } from "$lib/features/settings/store";
import type { AppSettings, PlatformDef, RuntimeOs } from "$lib/features/settings/types";
import { getPlatform } from "$lib/shared/platform";
import { getPlatformDefinition } from "$lib/platforms/registry";
import { DEFAULT_LOCALE } from "$lib/i18n";
import { getThemeDefinition } from "$lib/theme/themes";

export function isPlatformCompatibleWithOs(platform: PlatformDef | undefined, runtimeOs: RuntimeOs): boolean {
  if (!platform) return false;
  return platform.supportedOs.includes(runtimeOs);
}

export function isPlatformUsable(platformId: string, runtimeOs: RuntimeOs): boolean {
  const platform = ALL_PLATFORMS.find((entry) => entry.id === platformId);
  return Boolean(platform?.implemented && isPlatformCompatibleWithOs(platform, runtimeOs));
}

export function getInitialActiveTab(settings: AppSettings, runtimeOs: RuntimeOs): string {
  if (settings.enabledPlatforms.includes(settings.defaultPlatformId) && isPlatformUsable(settings.defaultPlatformId, runtimeOs)) {
    return settings.defaultPlatformId;
  }
  const firstEnabledUsable = settings.enabledPlatforms.find((platformId) => isPlatformUsable(platformId, runtimeOs));
  if (firstEnabledUsable) return firstEnabledUsable;
  const firstUsable = ALL_PLATFORMS.find((platform) => isPlatformUsable(platform.id, runtimeOs));
  if (firstUsable) return firstUsable.id;
  return settings.enabledPlatforms[0] || "steam";
}

export function createPlatformShellState() {
  const startupSettings = getSettings();
  let settings = $state(startupSettings);
  let runtimeOs = $state<RuntimeOs>("unknown");
  let adapterEpoch = $state(0);
  let locale = $derived(settings.language ?? DEFAULT_LOCALE);
  let enabledPlatforms = $derived<PlatformDef[]>(
    ALL_PLATFORMS.filter((platform) => settings.enabledPlatforms.includes(platform.id))
  );
  let compatiblePlatforms = $derived<PlatformDef[]>(
    ALL_PLATFORMS.filter((platform) => isPlatformUsable(platform.id, runtimeOs))
  );
  let activeTab = $state(getInitialActiveTab(startupSettings, "unknown"));
  let activePlatformDef = $derived(getPlatformDefinition(activeTab));
  let activeTabUsable = $derived(isPlatformUsable(activeTab, runtimeOs));
  let unavailablePlatformIds = $derived.by(() => {
    const ids = new Set<string>();
    for (const platform of enabledPlatforms) {
      if (!isPlatformUsable(platform.id, runtimeOs)) {
        ids.add(platform.id);
      }
    }
    return ids;
  });
  let accentColor = $derived(getPlatformDefinition(activeTab)?.accent || "#3b82f6");
  let activeTheme = $derived(getThemeDefinition(settings.themeId));
  let uiZoomFactor = $derived(Math.min(1.5, Math.max(0.75, settings.uiScalePercent / 100)));
  let appStageStyle = $derived.by(() => {
    const zoom = uiZoomFactor;
    if (Math.abs(zoom - 1) < 0.0001) return "";
    return `transform: scale(${zoom}); transform-origin: top left; width: calc(100% / ${zoom}); height: calc(100% / ${zoom});`;
  });
  let adapter = $derived.by(() => {
    adapterEpoch;
    return activeTabUsable ? getPlatform(activeTab) : undefined;
  });

  function refreshSettings() {
    settings = getSettings();
  }

  function setRuntimeOs(next: RuntimeOs) {
    runtimeOs = next;
  }

  function setActiveTab(next: string) {
    activeTab = next;
  }

  function adapterRegistryChanged() {
    adapterEpoch += 1;
  }

  function ensureActiveTab(): boolean {
    if (isPlatformUsable(activeTab, runtimeOs)) return false;
    const fallbackTab = getInitialActiveTab(settings, runtimeOs);
    if (fallbackTab === activeTab) return false;
    activeTab = fallbackTab;
    return true;
  }

  return {
    get settings() { return settings; },
    set settings(next: AppSettings) { settings = next; },
    get runtimeOs() { return runtimeOs; },
    get locale() { return locale; },
    get enabledPlatforms() { return enabledPlatforms; },
    get compatiblePlatforms() { return compatiblePlatforms; },
    get activeTab() { return activeTab; },
    get activePlatformDef() { return activePlatformDef; },
    get activeTabUsable() { return activeTabUsable; },
    get unavailablePlatformIds() { return unavailablePlatformIds; },
    get accentColor() { return accentColor; },
    get activeTheme() { return activeTheme; },
    get uiZoomFactor() { return uiZoomFactor; },
    get appStageStyle() { return appStageStyle; },
    get adapter() { return adapter; },
    refreshSettings,
    setRuntimeOs,
    setActiveTab,
    adapterRegistryChanged,
    ensureActiveTab,
  };
}
