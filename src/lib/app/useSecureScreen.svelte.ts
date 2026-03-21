import { hashPinCode, sanitizePinDigits, isValidPinHash } from "$lib/shared/pin";
import type { AppSettings } from "$lib/features/settings/types";
import type { MessageKey, TranslationParams } from "$lib/i18n";

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
  getIsAccountSelectionView: () => boolean;
  getAppVersion: () => string;
  onCloseContextMenu: () => void;
  t: (key: MessageKey, params?: TranslationParams) => string;
};

const PIN_CODE_LENGTH = 4;
const PIN_FAILURE_DELAY_MS = 1200;
const AFK_TEXT_FADE_MS = 900;
const AFK_TEXT_REVEAL_DELAY_MS = 2500;

export function createSecureScreenController({
  blur,
  windowActivity,
  getSettings,
  getIsAccountSelectionView,
  getAppVersion,
  onCloseContextMenu,
  t,
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
    if (!settings.pinEnabled || !hasValidPinCode) {
      isPinLocked = false;
      isPinRetryLocked = false;
      pinAttempt = "";
      pinError = "";
    }
  });

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
    if (!isValidPinHash(expectedPinHash)) {
      isPinLocked = false;
      return;
    }
    const attemptPin = sanitizePinDigits(pinAttempt);
    if (attemptPin.length !== PIN_CODE_LENGTH || isPinRetryLocked) return;
    isPinUnlocking = true;
    pinError = "";
    const attemptHash = await hashPinCode(attemptPin);
    if (!attemptHash) {
      isPinUnlocking = false;
      return;
    }
    if (attemptHash !== expectedPinHash) {
      isPinUnlocking = false;
      isPinRetryLocked = true;
      pinError = t("pin.invalid");
      pinAttempt = "";
      if (pinRetryTimer) {
        clearTimeout(pinRetryTimer);
      }
      pinRetryTimer = setTimeout(() => {
        pinRetryTimer = null;
        isPinRetryLocked = false;
        setTimeout(() => pinInputRef?.focus(), 0);
      }, PIN_FAILURE_DELAY_MS);
      return;
    }
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
    if (isPinLocked) {
      isPinRetryLocked = false;
      pinAttempt = "";
      pinError = "";
      setTimeout(() => pinInputRef?.focus(), 0);
    }
  }

  function handleAppDestroyed() {
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
