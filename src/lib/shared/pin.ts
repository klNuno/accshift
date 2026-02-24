const PIN_CODE_LENGTH = 4;
const PIN_HASH_RE = /^[a-f0-9]{64}$/i;

export function sanitizePinDigits(value: string): string {
  return value.replace(/\D/g, "").slice(0, PIN_CODE_LENGTH);
}

export function isValidPinCode(value: string): boolean {
  return sanitizePinDigits(value).length === PIN_CODE_LENGTH;
}

export function isValidPinHash(value: string): boolean {
  return PIN_HASH_RE.test(value);
}

export async function hashPinCode(pinCode: string): Promise<string> {
  const normalized = sanitizePinDigits(pinCode);
  if (normalized.length !== PIN_CODE_LENGTH) return "";
  const bytes = new TextEncoder().encode(normalized);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}
