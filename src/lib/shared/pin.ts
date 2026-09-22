const PIN_CODE_LENGTH = 4;
const PBKDF2_ITERATIONS = 100_000;
const SALT_BYTES = 16;
const HASH_BYTES = 32;

/** New PBKDF2 format: 32-char salt hex + ":" + 64-char hash hex */
const PIN_HASH_RE = /^[a-f0-9]{32}:[a-f0-9]{64}$/i;
/** Legacy SHA-256 format (no salt), accepted for migration */
const LEGACY_HASH_RE = /^[a-f0-9]{64}$/i;

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.substring(i, i + 2), 16);
  }
  return bytes;
}

function constantTimeEqual(a: string, b: string): boolean {
  const maxLength = Math.max(a.length, b.length);
  let diff = a.length ^ b.length;
  for (let i = 0; i < maxLength; i++) {
    diff |= (a.charCodeAt(i) || 0) ^ (b.charCodeAt(i) || 0);
  }
  return diff === 0;
}

async function pbkdf2Derive(pin: string, salt: Uint8Array): Promise<string> {
  const keyMaterial = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(pin),
    "PBKDF2",
    false,
    ["deriveBits"],
  );
  const derived = await crypto.subtle.deriveBits(
    { name: "PBKDF2", salt: salt as BufferSource, iterations: PBKDF2_ITERATIONS, hash: "SHA-256" },
    keyMaterial,
    HASH_BYTES * 8,
  );
  return bytesToHex(new Uint8Array(derived));
}

export function sanitizePinDigits(value: string): string {
  return value.replace(/\D/g, "").slice(0, PIN_CODE_LENGTH);
}

export function isValidPinHash(value: string): boolean {
  return PIN_HASH_RE.test(value) || LEGACY_HASH_RE.test(value);
}

/** Hash a PIN with PBKDF2 and a random salt. Returns "salt:hash" format. */
export async function hashPinCode(pinCode: string): Promise<string> {
  const normalized = sanitizePinDigits(pinCode);
  if (normalized.length !== PIN_CODE_LENGTH) return "";
  const salt = crypto.getRandomValues(new Uint8Array(SALT_BYTES));
  const hash = await pbkdf2Derive(normalized, salt);
  return `${bytesToHex(salt)}:${hash}`;
}

export type PinVerification = {
  /** True when the attempt matches the stored hash. */
  matches: boolean;
  /**
   * A PBKDF2 replacement for a legacy hash the attempt just cleared, to be
   * persisted by the caller. Null on a failed attempt and on a hash that is
   * already PBKDF2, so nothing is rewritten in those cases.
   */
  rehashed: string | null;
};

/**
 * Verify a PIN attempt against a stored hash. Handles both PBKDF2 and legacy
 * unsalted SHA-256.
 *
 * A legacy hash that verifies is upgraded on the spot: the returned
 * `rehashed` is the same PIN under the current PBKDF2 path, and persisting it
 * is what ends the migration. Without that write the unsalted form would be
 * accepted for ever, which is the whole point of accepting it at all.
 */
export async function verifyPinCode(pinCode: string, storedHash: string): Promise<PinVerification> {
  const normalized = sanitizePinDigits(pinCode);
  if (normalized.length !== PIN_CODE_LENGTH) return { matches: false, rehashed: null };

  if (!storedHash.includes(":") && LEGACY_HASH_RE.test(storedHash)) {
    const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(normalized));
    const matches = constantTimeEqual(bytesToHex(new Uint8Array(digest)), storedHash.toLowerCase());
    if (!matches) return { matches: false, rehashed: null };
    return { matches: true, rehashed: await hashPinCode(normalized) };
  }

  const colonIdx = storedHash.indexOf(":");
  if (colonIdx === -1) return { matches: false, rehashed: null };
  const salt = hexToBytes(storedHash.slice(0, colonIdx));
  const expectedHash = storedHash.slice(colonIdx + 1);
  const attemptHash = await pbkdf2Derive(normalized, salt);
  return {
    matches: constantTimeEqual(attemptHash, expectedHash.toLowerCase()),
    rehashed: null,
  };
}
