import { invoke } from "@tauri-apps/api/core";
import { hashPinCode, isValidPinHash, verifyPinCode } from "./pin";

/**
 * Start of the error every switch command returns while a PIN is set and the
 * backend session is locked (`PIN_LOCKED_MESSAGE` in
 * `crates/accshift-core/src/pin.rs`). Errors cross IPC as bare strings, so
 * the prefix is the type.
 */
export const PIN_LOCKED_PREFIX = "pin_locked:";

export function isPinLockedError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return message.startsWith(PIN_LOCKED_PREFIX);
}

type PinLockedListener = () => void;
const pinLockedListeners = new Set<PinLockedListener>();

/** Called whenever a command was refused because the session is locked. */
export function onPinLockedError(listener: PinLockedListener): () => void {
  pinLockedListeners.add(listener);
  return () => {
    pinLockedListeners.delete(listener);
  };
}

/**
 * Tell the lock screen that the backend refused a command for want of the
 * PIN. True when `error` was that refusal, so the caller can skip its generic
 * failure message.
 */
export function reportPinLockedError(error: unknown): boolean {
  if (!isPinLockedError(error)) return false;
  for (const listener of pinLockedListeners) listener();
  return true;
}

type BackendUnlockResult = {
  status: "unlocked" | "invalid" | "retry_later" | "not_configured";
  legacy: boolean;
  retryAfterMs: number;
};

export type PinCheck =
  | {
      status: "match";
      /** PBKDF2 replacement for a legacy hash the code just cleared, to persist. */
      rehashed: string | null;
    }
  | { status: "invalid"; retryAfterMs: number }
  | { status: "retry_later"; retryAfterMs: number };

/**
 * Check `code` with the backend, which unlocks its session on a match: the
 * switch commands answer only once it has.
 *
 * The backend reads the PIN from disk. When it finds none (a PIN typed in
 * Settings a moment ago, not saved yet) or cannot read the store, it gates no
 * switch on a PIN, or refuses every one on its own. The screen then follows the
 * hash this window holds, as it did before the backend had a say.
 */
export async function unlockPinSession(code: string, storedHash: string): Promise<PinCheck> {
  let result: BackendUnlockResult | null = null;
  try {
    result = await invoke<BackendUnlockResult>("pin_unlock", { code });
  } catch (error) {
    console.error("[pin] pin_unlock failed:", error);
  }
  if (result?.status === "unlocked") {
    return { status: "match", rehashed: result.legacy ? await hashPinCode(code) : null };
  }
  if (result?.status === "invalid" || result?.status === "retry_later") {
    return { status: result.status, retryAfterMs: result.retryAfterMs };
  }
  if (!isValidPinHash(storedHash)) {
    // No PIN here either: nothing to protect.
    return { status: "match", rehashed: null };
  }
  const { matches, rehashed } = await verifyPinCode(code, storedHash);
  return matches ? { status: "match", rehashed } : { status: "invalid", retryAfterMs: 0 };
}

/** Lock the backend session. Fire and forget: the screen locks either way. */
export function lockPinSession(): void {
  void invoke("pin_lock").catch((error) => {
    console.error("[pin] pin_lock failed:", error);
  });
}

/** Shortest wait after a wrong code, whatever the backend says. */
export const PIN_FAILURE_DELAY_MS = 1200;

/**
 * How long a screen refuses input after `check` failed, and whether to say
 * the attempts ran out rather than "wrong PIN". The backend's wait wins when
 * it is longer than the local one.
 */
export function pinFailureWait(check: Exclude<PinCheck, { status: "match" }>): {
  waitMs: number;
  tooManyAttempts: boolean;
} {
  return {
    waitMs: Math.max(PIN_FAILURE_DELAY_MS, check.retryAfterMs),
    tooManyAttempts: check.status === "retry_later" || check.retryAfterMs > PIN_FAILURE_DELAY_MS,
  };
}
