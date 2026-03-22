import type { AppSettings } from "$lib/features/settings/types";
import { trackDependencies } from "$lib/shared/trackDependencies";

const UI_SCALE_STEP_PERCENT = 5;
const UI_SCALE_MIN_PERCENT = 75;
const UI_SCALE_MAX_PERCENT = 150;
const WHEEL_ZOOM_THRESHOLD = 80;

type UiScaleOptions = {
  /** Returns the live reactive settings object (shell.settings). */
  getSettings: () => AppSettings;
  /** Loads a fresh settings snapshot and persists updates to storage. */
  saveSettings: (mutate: (settings: AppSettings) => void) => void;
  getGridLayout: () => { queueCalculatePadding: () => void };
};

export function createUiScale({ getSettings, saveSettings, getGridLayout }: UiScaleOptions) {
  let zoomPersistTimer: ReturnType<typeof setTimeout> | null = null;
  let wheelZoomAccumulator = 0;

  function clampUiScalePercent(value: number): number {
    const rounded = Math.round(value / UI_SCALE_STEP_PERCENT) * UI_SCALE_STEP_PERCENT;
    return Math.min(UI_SCALE_MAX_PERCENT, Math.max(UI_SCALE_MIN_PERCENT, rounded));
  }

  function persistUiScalePercent(value: number) {
    const next = clampUiScalePercent(value);
    saveSettings((latest) => {
      if (latest.uiScalePercent === next) return;
      latest.uiScalePercent = next;
    });
  }

  function queuePersistUiScalePercent(value: number) {
    if (zoomPersistTimer) clearTimeout(zoomPersistTimer);
    zoomPersistTimer = setTimeout(() => {
      persistUiScalePercent(value);
      zoomPersistTimer = null;
    }, 180);
  }

  function setUiScalePercent(value: number) {
    const settings = getSettings();
    const next = clampUiScalePercent(value);
    if (next === settings.uiScalePercent) return;
    settings.uiScalePercent = next;
    queuePersistUiScalePercent(next);
  }

  $effect(() => {
    trackDependencies(getSettings().uiScalePercent);
    getGridLayout().queueCalculatePadding();
  });

  function handleCtrlWheelZoom(e: WheelEvent) {
    if (!e.ctrlKey) {
      wheelZoomAccumulator = 0;
      return;
    }
    e.preventDefault();
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? window.innerHeight : 1;
    wheelZoomAccumulator += e.deltaY * unit;
    if (Math.abs(wheelZoomAccumulator) < WHEEL_ZOOM_THRESHOLD) return;
    const direction = wheelZoomAccumulator < 0 ? 1 : -1;
    wheelZoomAccumulator = 0;
    setUiScalePercent(getSettings().uiScalePercent + direction * UI_SCALE_STEP_PERCENT);
  }

  function handleZoomKeydown(e: KeyboardEvent) {
    if (!e.ctrlKey && !e.metaKey) return;
    if (e.key !== "0") return;
    e.preventDefault();
    wheelZoomAccumulator = 0;
    setUiScalePercent(100);
  }

  function destroy() {
    if (zoomPersistTimer) {
      clearTimeout(zoomPersistTimer);
      zoomPersistTimer = null;
    }
  }

  return {
    handleCtrlWheelZoom,
    handleZoomKeydown,
    destroy,
  };
}
