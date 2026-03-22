import { addToast } from "$lib/features/notifications/store.svelte";
import type { MessageKey, TranslationParams } from "$lib/i18n";

type SettingsComponentType = (typeof import("$lib/features/settings/Settings.svelte"))["default"];

type SettingsPanelDeps = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  onClose: () => void;
};

export function createSettingsPanel({ t, onClose }: SettingsPanelDeps) {
  let showSettings = $state(false);
  let SettingsPanel = $state<SettingsComponentType | null>(null);
  let settingsLoadPromise: Promise<void> | null = null;

  function loadComponent() {
    if (SettingsPanel) return Promise.resolve();
    if (!settingsLoadPromise) {
      settingsLoadPromise = import("$lib/features/settings/Settings.svelte")
        .then((mod) => {
          SettingsPanel = mod.default;
        })
        .catch((error) => {
          console.error("Failed to load settings panel:", error);
          addToast(t("toast.failedLoadSettingsPanel"));
          settingsLoadPromise = null;
        });
    }
    return settingsLoadPromise;
  }

  function open() {
    showSettings = true;
    void loadComponent();
  }

  function close() {
    showSettings = false;
    onClose();
  }

  return {
    get showSettings() {
      return showSettings;
    },
    set showSettings(value: boolean) {
      showSettings = value;
    },
    get SettingsPanel() {
      return SettingsPanel;
    },
    open,
    close,
    loadComponent,
  };
}
