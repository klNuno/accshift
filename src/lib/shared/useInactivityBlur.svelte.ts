import { getSettings } from "../features/settings/store";

export function createInactivityBlur() {
  let lastActivity = Date.now();
  let isBlurred = $state(false);
  let intervalId: number | undefined;
  let lastActivityUpdate = 0;

  function resetActivity() {
    const now = Date.now();
    // Throttle: update lastActivity at most once per second, except for unblur which is immediate
    if (isBlurred) {
      lastActivity = now;
      isBlurred = false;
    } else if (now - lastActivityUpdate > 1000) {
      lastActivity = now;
      lastActivityUpdate = now;
    }
  }

  function start() {
    stop();
    const threshold = getSettings().inactivityBlurSeconds;
    if (threshold === 0) {
      isBlurred = false;
      return;
    }
    intervalId = setInterval(() => {
      const s = getSettings();
      if (s.inactivityBlurSeconds === 0) {
        isBlurred = false;
        return;
      }
      if (!isBlurred && Date.now() - lastActivity > s.inactivityBlurSeconds * 1000) {
        isBlurred = true;
      }
    }, 1000) as unknown as number;
  }

  function stop() {
    if (intervalId !== undefined) {
      clearInterval(intervalId);
      intervalId = undefined;
    }
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
