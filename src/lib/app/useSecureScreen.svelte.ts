import { sanitizePinDigits, isValidPinHash } from "$lib/shared/pin";
import {
  lockPinSession,
  onPinLockedError,
  pinFailureWait,
  unlockPinSession,
  type PinCheck,
} from "$lib/shared/pinSession";
import type { AppSettings } from "$lib/features/settings/types";
import type { MessageKey, TranslationParams } from "$lib/i18n";

/** The backend half of the lock. Injected by tests. */
export type PinSessionPort = {
  unlock: (code: string, storedHash: string) => Promise<PinCheck>;
  lock: () => void;
  onLockedError: (listener: () => void) => () => void;
};

const BACKEND_PIN_SESSION: PinSessionPort = {
  unlock: unlockPinSession,
  lock: lockPinSession,
  onLockedError: onPinLockedError,
};

type SecureScreenDeps = {
  blur: {
    get isBlurred(): boolean;
    resetActivity: () => void;
    start: () => void;
    stop: () => void;
    attachListeners: () => void;
    detachListeners: () => void;
  };
  windowActivity: {
    get isForeground(): boolean;
    get isMinimized(): boolean;
    get isPageVisible(): boolean;
  };
  getSettings: () => AppSettings;
  /**
   * The AFK screen without a PIN only covers the account grid. The PIN lock
   * does not ask: it engages in every view, Settings included.
   */
  getIsAccountSelectionView: () => boolean;
  getAppVersion: () => string;
  onCloseContextMenu: () => void;
  /**
   * Write a PBKDF2 hash that just replaced a legacy unsalted one. Called at
   * most once per legacy PIN, right after it unlocked the screen.
   */
  persistPinHash: (hash: string) => void;
  t: (key: MessageKey, params?: TranslationParams) => string;
  pinSession?: PinSessionPort;
};

const PIN_CODE_LENGTH = 4;
const AFK_TEXT_FADE_MS = 900;
const AFK_TEXT_REVEAL_DELAY_MS = 2500;

export function createSecureScreenController({
  blur,
  windowActivity,
  getSettings,
  getIsAccountSelectionView,
  getAppVersion,
  onCloseContextMenu,
  persistPinHash,
  t,
  pinSession = BACKEND_PIN_SESSION,
}: SecureScreenDeps) {
  const startupPinLocked = Boolean(
    getSettings().pinEnabled && isValidPinHash(getSettings().pinHash || ""),
  );

  let isPinLocked = $state(startupPinLocked);
  let isPinUnlocking = $state(false);
  let isPinRetryLocked = $state(false);
  let pinAttempt = $state("");
  let pinError = $state("");
  let pinInputRef = $state<HTMLInputElement | null>(null);
  let pinRetryTimer: ReturnType<typeof setTimeout> | null = null;
  // The backend refused a switch for want of the PIN. The screen stays locked
  // on that word alone, even when this window holds no PIN of its own (the
  // store changed under it): only a code the backend accepts clears it.
  let backendLocked = $state(false);
  let stopPinLockedListener: (() => void) | null = null;
  let afkListenersAttached = $state(false);
  let afkWaveActive = $state(false);
  let afkWaveStopTimer: ReturnType<typeof setTimeout> | null = null;

  let windowForeground = $derived(windowActivity.isForeground);
  let windowRenderable = $derived(windowActivity.isPageVisible && !windowActivity.isMinimized);
  let windowMinimized = $derived(windowActivity.isMinimized);
  let renderSuspended = $derived(getSettings().suspendGraphicsWhenMinimized && windowMinimized);
  let inactivityEnabled = $derived(getSettings().inactivityBlurSeconds > 0);
  let isObscured = $derived(
    (inactivityEnabled && blur.isBlurred && getIsAccountSelectionView()) ||
      isPinLocked ||
      isPinUnlocking ||
      isPinRetryLocked,
  );
  let afkOverlayVisible = $derived(
    inactivityEnabled &&
      blur.isBlurred &&
      getIsAccountSelectionView() &&
      !isPinLocked &&
      !isPinUnlocking &&
      !isPinRetryLocked &&
      windowRenderable &&
      !renderSuspended,
  );
  let motionPaused = $derived(!windowRenderable || renderSuspended);
  let afkVersionLabel = $derived(afkOverlayVisible && getAppVersion() ? getAppVersion() : null);

  $effect(() => {
    const visible = afkOverlayVisible;
    if (afkWaveStopTimer) {
      clearTimeout(afkWaveStopTimer);
      afkWaveStopTimer = null;
    }
    if (visible) {
      onCloseContextMenu();
      afkWaveActive = true;
      return;
    }
    if (!afkWaveActive) return;
    afkWaveStopTimer = setTimeout(() => {
      afkWaveActive = false;
      afkWaveStopTimer = null;
    }, AFK_TEXT_FADE_MS);
  });

  $effect(() => {
    if (renderSuspended) {
      onCloseContextMenu();
    }
  });

  $effect(() => {
    if (renderSuspended) {
      if (afkListenersAttached) {
        blur.detachListeners();
        afkListenersAttached = false;
      }
      return;
    }
    if (!afkListenersAttached) {
      blur.attachListeners();
      afkListenersAttached = true;
    }
  });

  $effect(() => {
    const settings = getSettings();
    const hasValidPinCode = isValidPinHash(settings.pinHash || "");
    if ((!settings.pinEnabled || !hasValidPinCode) && !backendLocked) {
      isPinLocked = false;
      isPinRetryLocked = false;
      pinAttempt = "";
      pinError = "";
    }
  });

  $effect(() => {
    const settings = getSettings();
    if (
      !blur.isBlurred ||
      !settings.pinEnabled ||
      !isValidPinHash(settings.pinHash || "") ||
      isPinLocked ||
      isPinUnlocking
    ) {
      return;
    }

    engageLock();
  });

  /** Lock the screen and the backend session together. */
  function engageLock({ tellBackend = true } = {}) {
    if (tellBackend) pinSession.lock();
    isPinLocked = true;
    isPinRetryLocked = false;
    pinAttempt = "";
    pinError = "";
    onCloseContextMenu();
    setTimeout(() => pinInputRef?.focus(), 0);
  }

  function handleBackendLocked() {
    backendLocked = true;
    if (isPinLocked || isPinUnlocking) return;
    // The backend is locked already: no need to tell it.
    engageLock({ tellBackend: false });
  }

  $effect(() => {
    const sanitizedAttempt = sanitizePinDigits(pinAttempt);
    if (sanitizedAttempt !== pinAttempt) {
      pinAttempt = sanitizedAttempt;
      return;
    }
    if (!isPinLocked || isPinUnlocking || isPinRetryLocked) return;
    if (sanitizedAttempt.length === PIN_CODE_LENGTH) {
      void unlockWithPin();
    }
  });

  async function unlockWithPin() {
    const expectedPinHash = getSettings().pinHash || "";
    if (!isValidPinHash(expectedPinHash) && !backendLocked) {
      isPinLocked = false;
      return;
    }
    const attemptPin = sanitizePinDigits(pinAttempt);
    if (attemptPin.length !== PIN_CODE_LENGTH || isPinRetryLocked) return;
    isPinUnlocking = true;
    pinError = "";
    // The backend checks the code and unlocks the switch commands. It also
    // rate limits wrong codes, so its wait wins over the local one.
    const check = await pinSession.unlock(attemptPin, expectedPinHash);
    if (check.status !== "match") {
      const { waitMs, tooManyAttempts } = pinFailureWait(check);
      isPinUnlocking = false;
      isPinRetryLocked = true;
      pinError = tooManyAttempts
        ? t("pin.tooManyAttempts", { seconds: Math.ceil(waitMs / 1000) })
        : t("pin.invalid");
      pinAttempt = "";
      if (pinRetryTimer) {
        clearTimeout(pinRetryTimer);
      }
      pinRetryTimer = setTimeout(() => {
        pinRetryTimer = null;
        isPinRetryLocked = false;
        setTimeout(() => pinInputRef?.focus(), 0);
      }, waitMs);
      return;
    }
    backendLocked = false;
    // The unlock succeeded against the old unsalted hash. Store the PBKDF2
    // one now, while the digits are still in hand, so the next unlock (here
    // or in the CLI) runs the salted path.
    if (check.rehashed) persistPinHash(check.rehashed);
    pinAttempt = "";
    setTimeout(() => {
      isPinLocked = false;
      isPinUnlocking = false;
      blur.resetActivity();
    }, 240);
  }

  function handleSettingsClosed() {
    blur.start();
    if (!afkListenersAttached) {
      blur.attachListeners();
      afkListenersAttached = true;
    }
  }

  function handleAppMounted() {
    blur.start();
    blur.attachListeners();
    afkListenersAttached = true;
    stopPinLockedListener?.();
    stopPinLockedListener = pinSession.onLockedError(handleBackendLocked);
    if (isPinLocked) {
      // The backend locks itself at start when the store holds a PIN. Say it
      // anyway: this window may be a reload of an unlocked session.
      pinSession.lock();
      isPinRetryLocked = false;
      pinAttempt = "";
      pinError = "";
      setTimeout(() => pinInputRef?.focus(), 0);
    }
  }

  function handleAppDestroyed() {
    stopPinLockedListener?.();
    stopPinLockedListener = null;
    if (afkWaveStopTimer) {
      clearTimeout(afkWaveStopTimer);
      afkWaveStopTimer = null;
    }
    if (pinRetryTimer) {
      clearTimeout(pinRetryTimer);
      pinRetryTimer = null;
    }
    if (afkListenersAttached) {
      blur.detachListeners();
    }
    blur.stop();
  }

  function setPinInputRef(node: HTMLInputElement | null) {
    pinInputRef = node;
  }

  function setPinAttempt(value: string) {
    pinAttempt = value;
  }

  return {
    get isPinLocked() {
      return isPinLocked;
    },
    get isPinUnlocking() {
      return isPinUnlocking;
    },
    get isPinRetryLocked() {
      return isPinRetryLocked;
    },
    get pinAttempt() {
      return pinAttempt;
    },
    get pinError() {
      return pinError;
    },
    get windowForeground() {
      return windowForeground;
    },
    get renderSuspended() {
      return renderSuspended;
    },
    get isObscured() {
      return isObscured;
    },
    get afkOverlayVisible() {
      return afkOverlayVisible;
    },
    get motionPaused() {
      return motionPaused;
    },
    get afkWaveActive() {
      return afkWaveActive;
    },
    get afkVersionLabel() {
      return afkVersionLabel;
    },
    get pinCodeLength() {
      return PIN_CODE_LENGTH;
    },
    get afkTextRevealDelayMs() {
      return AFK_TEXT_REVEAL_DELAY_MS;
    },
    handleSettingsClosed,
    handleAppMounted,
    handleAppDestroyed,
    setPinInputRef,
    setPinAttempt,
  };
}
