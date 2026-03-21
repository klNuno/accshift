import {
  CLIENT_STORE_ACCOUNT_CARD_NOTES,
  getClientStoreRevision,
  getClientStoreValue,
  setClientStoreValue,
} from "$lib/storage/clientStorage";

const MAX_NOTE_LENGTH = 180;

type AccountNoteMap = Record<string, string>;
let cachedMap: AccountNoteMap | null = null;
let cachedRevision = -1;

function sanitizeNote(value: string): string {
  const withoutControls = value.replace(/\p{Cc}/gu, " ");
  return withoutControls.trim().slice(0, MAX_NOTE_LENGTH);
}

function sanitizeMap(value: unknown): AccountNoteMap {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const out: AccountNoteMap = {};
  for (const [key, rawNote] of Object.entries(value as Record<string, unknown>)) {
    if (typeof key !== "string" || key.trim().length === 0) continue;
    if (typeof rawNote !== "string") continue;
    const note = sanitizeNote(rawNote);
    if (!note) continue;
    out[key] = note;
  }
  return out;
}

function readMap(): AccountNoteMap {
  const revision = getClientStoreRevision(CLIENT_STORE_ACCOUNT_CARD_NOTES);
  if (cachedMap && cachedRevision === revision) return cachedMap;
  try {
    const raw = getClientStoreValue<unknown>(CLIENT_STORE_ACCOUNT_CARD_NOTES);
    if (raw == null) {
      cachedMap = {};
      cachedRevision = revision;
      return cachedMap;
    }
    cachedMap = sanitizeMap(raw);
    cachedRevision = revision;
    return cachedMap;
  } catch {
    cachedMap = {};
    cachedRevision = revision;
    return cachedMap;
  }
}

function writeMap(data: AccountNoteMap) {
  cachedMap = data;
  setClientStoreValue(CLIENT_STORE_ACCOUNT_CARD_NOTES, data);
  cachedRevision = getClientStoreRevision(CLIENT_STORE_ACCOUNT_CARD_NOTES);
}

export function getAccountCardNote(accountId: string): string {
  return readMap()[accountId] ?? "";
}

export function setAccountCardNote(accountId: string, note: string) {
  const data = readMap();
  const safe = sanitizeNote(note);
  if (!safe) {
    delete data[accountId];
  } else {
    data[accountId] = safe;
  }
  writeMap(data);
}

export function clearAccountCardNote(accountId: string) {
  const data = readMap();
  delete data[accountId];
  writeMap(data);
}
