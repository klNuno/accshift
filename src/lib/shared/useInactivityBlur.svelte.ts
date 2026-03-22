import { getSettings } from "../features/settings/store";

export function createInactivityBlur() {
  const POINTER_ACTIVITY_THROTTLE_MS = 200;
  let lastActivity = Date.now();
  let lastPointerActivity = 0;
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

  function resetPointerActivity() {
    const now = Date.now();
    if (now - lastPointerActivity < POINTER_ACTIVITY_THROTTLE_MS) {
      return;
    }
    lastPointerActivity = now;
    resetActivity();
  }

  function start() {
    stop();
    const threshold = getSettings().inactivityBlurSeconds;
    if (threshold === 0) {
      isBlurred = false;
      return;
    }
    lastActivity = Date.now();
    lastPointerActivity = lastActivity;
    scheduleBlurCheck();
  }

  function stop() {
    clearBlurTimeout();
  }

  function attachListeners() {
    document.addEventListener("mousemove", resetPointerActivity, { passive: true });
    document.addEventListener("mousedown", resetActivity, { passive: true });
    document.addEventListener("keydown", resetActivity, { passive: true });
  }

  function detachListeners() {
    document.removeEventListener("mousemove", resetPointerActivity);
    document.removeEventListener("mousedown", resetActivity);
    document.removeEventListener("keydown", resetActivity);
  }

  return {
    get isBlurred() {
      return isBlurred;
    },
    resetActivity,
    start,
    stop,
    attachListeners,
    detachListeners,
  };
}
