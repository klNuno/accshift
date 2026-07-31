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
    // Nothing is painted while the page is hidden, so a scan cannot change
    // anything the user sees. Skipping it drops the whole idle cost: each
    // tick is a full process-table refresh in the backend. The detected state
    // is deliberately left alone, so a stream running before the window was
    // hidden keeps the overlay armed, and returning to visible polls at once.
    if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
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

  // Catches up as soon as the window is shown again, so a stream started while
  // it was hidden is detected on the next frame rather than up to a tick later.
  function handleVisibilityChange() {
    if (document.visibilityState === "visible") void poll();
  }

  function start() {
    if (pollTimer) return;
    void poll();
    pollTimer = setInterval(() => void poll(), POLL_INTERVAL_MS);
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", handleVisibilityChange);
    }
  }

  function stop() {
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
    if (typeof document !== "undefined") {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
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
