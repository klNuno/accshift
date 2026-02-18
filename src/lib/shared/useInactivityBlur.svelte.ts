import { getSettings } from "../features/settings/store";

export function createInactivityBlur() {
  let lastActivity = Date.now();
  let isBlurred = $state(false);
  let blurTimeoutId: number | undefined;

  function clearBlurTimeout() {
    if (blurTimeoutId !== undefined) {
      clearTimeout(blurTimeoutId);
      blurTimeoutId = undefined;
    }
  }

  function scheduleBlurCheck() {
    clearBlurTimeout();
    const thresholdMs = getSettings().inactivityBlurSeconds * 1000;
    if (thresholdMs <= 0) {
      isBlurred = false;
      return;
    }

    const elapsedMs = Date.now() - lastActivity;
    const remainingMs = Math.max(0, thresholdMs - elapsedMs);
    blurTimeoutId = setTimeout(() => {
      const settings = getSettings();
      if (settings.inactivityBlurSeconds <= 0) {
        isBlurred = false;
        return;
      }

      if (Date.now() - lastActivity >= settings.inactivityBlurSeconds * 1000) {
        isBlurred = true;
        return;
      }

      scheduleBlurCheck();
    }, remainingMs) as unknown as number;
  }

  function resetActivity() {
    lastActivity = Date.now();
    if (isBlurred) {
      isBlurred = false;
    }
    scheduleBlurCheck();
  }

  function start() {
    stop();
    const threshold = getSettings().inactivityBlurSeconds;
    if (threshold === 0) {
      isBlurred = false;
      return;
    }
    lastActivity = Date.now();
    scheduleBlurCheck();
  }

  function stop() {
    clearBlurTimeout();
  }

  function attachListeners() {
    document.addEventListener("mousemove", resetActivity);
    document.addEventListener("mousedown", resetActivity);
    document.addEventListener("keydown", resetActivity);
  }

  function detachListeners() {
    document.removeEventListener("mousemove", resetActivity);
    document.removeEventListener("mousedown", resetActivity);
    document.removeEventListener("keydown", resetActivity);
  }

  return {
    get isBlurred() { return isBlurred; },
    resetActivity,
    start,
    stop,
    attachListeners,
    detachListeners,
  };
}
