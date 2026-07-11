import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, StreamerMode } from "$lib/features/settings/types";

type StreamerModeDeps = {
  getSettings: () => AppSettings;
  /** Persist a new streamer-mode setting (used by "disable permanently"). */
  setStreamerMode: (mode: StreamerMode) => void;
};

// Streaming software rarely starts and stops, so a few seconds of latency
// before the blur kicks in is fine and keeps the process scan cheap.
const POLL_INTERVAL_MS = 4000;

export function createStreamerModeController({ getSettings, setStreamerMode }: StreamerModeDeps) {
  let streamingDetected = $state(false);
  // "Disable for now" hides the overlay until the current stream session ends.
  // Reset once no streaming software is running, so reopening OBS re-triggers.
  let dismissedThisSession = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let polling = false;

  let enabled = $derived(getSettings().streamerMode === "auto");
  let active = $derived(enabled && streamingDetected && !dismissedThisSession);

  async function poll() {
    if (!enabled) {
      streamingDetected = false;
      return;
    }
    if (polling) return;
    polling = true;
    try {
      const detected = await invoke<boolean>("detect_streaming_software");
      if (detected && !streamingDetected) {
        void invoke("telemetry_track_streamer_mode").catch(() => {});
      }
      streamingDetected = detected;
      if (!detected) dismissedThisSession = false;
    } catch (e) {
      console.error("detect_streaming_software failed", e);
    } finally {
      polling = false;
    }
  }

  function start() {
    if (pollTimer) return;
    void poll();
    pollTimer = setInterval(() => void poll(), POLL_INTERVAL_MS);
  }

  function stop() {
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }

  function dismiss() {
    dismissedThisSession = true;
  }

  function disablePermanently() {
    dismissedThisSession = true;
    setStreamerMode("off");
  }

  return {
    get active() {
      return active;
    },
    start,
    stop,
    dismiss,
    disablePermanently,
  };
}
