import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, StreamerMode } from "$lib/features/settings/types";

type StreamerModeDeps = {
  getSettings: () => AppSettings;
  /** Persist a new streamer-mode setting (used by "disable permanently"). */
  setStreamerMode: (mode: StreamerMode) => void;
  /** True while the window is minimized or its page hidden: nothing to blur on screen. */
  isHidden?: () => boolean;
  now?: () => number;
};

// Streaming software rarely starts and stops, so a few seconds of latency
// before the blur kicks in is fine and keeps the process scan cheap.
const POLL_INTERVAL_MS = 4000;
// Each poll walks the whole process table on the backend. A hidden window
// shows nothing a stream could capture, so it checks far less often; the
// first tick after it comes back polls again.
const HIDDEN_POLL_INTERVAL_MS = 30_000;

export function createStreamerModeController({
  getSettings,
  setStreamerMode,
  isHidden = () => false,
  now = Date.now,
}: StreamerModeDeps) {
  let streamingDetected = $state(false);
  // "Disable for now" hides the overlay until the current stream session ends.
  // Reset once no streaming software is running, so reopening OBS re-triggers.
  let dismissedThisSession = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let polling = false;
  let lastPollAt = Number.NEGATIVE_INFINITY;

  let enabled = $derived(getSettings().streamerMode === "auto");
  let active = $derived(enabled && streamingDetected && !dismissedThisSession);

  async function poll() {
    if (!enabled) {
      streamingDetected = false;
      return;
    }
    if (polling) return;
    polling = true;
    lastPollAt = now();
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
    pollTimer = setInterval(() => {
      if (isHidden() && now() - lastPollAt < HIDDEN_POLL_INTERVAL_MS) return;
      void poll();
    }, POLL_INTERVAL_MS);
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
