import type { MessageKey, TranslationParams } from "$lib/i18n";

type SettingsComponentType = (typeof import("$lib/features/settings/Settings.svelte"))["default"];

type SettingsPanelControllerOptions = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  showToast: (message: string) => void;
  refreshSettings: () => void;
  setShowSettings: (value: boolean) => void;
  onAfterClose: () => void;
};

export function createSettingsPanelController({
  t,
  showToast,
  refreshSettings,
  setShowSettings,
  onAfterClose,
}: SettingsPanelControllerOptions) {
  let panel = $state<SettingsComponentType | null>(null);
  let loadPromise: Promise<void> | null = null;

  function loadComponent() {
    if (panel) return Promise.resolve();
    if (!loadPromise) {
      loadPromise = import("$lib/features/settings/Settings.svelte")
        .then((mod) => {
          panel = mod.default;
        })
        .catch((error) => {
          console.error("Failed to load settings panel:", error);
          showToast(t("toast.failedLoadSettingsPanel"));
          loadPromise = null;
        });
    }
    return loadPromise;
  }

  function handleClose() {
    setShowSettings(false);
    refreshSettings();
    onAfterClose();
  }

  function handleUpdated() {
    refreshSettings();
  }

  return {
    get panel() {
      return panel;
    },
    loadComponent,
    handleClose,
    handleUpdated,
  };
}
