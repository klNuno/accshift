const STORAGE_KEY = "accshift_account_card_notes";
const MAX_NOTE_LENGTH = 180;

type AccountNoteMap = Record<string, string>;
let cachedMap: AccountNoteMap | null = null;

function sanitizeNote(value: string): string {
  const withoutControls = value.replace(/[\u0000-\u001F\u007F]/g, " ");
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
  if (cachedMap) return cachedMap;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      cachedMap = {};
      return cachedMap;
    }
    cachedMap = sanitizeMap(JSON.parse(raw));
    return cachedMap;
  } catch {
    cachedMap = {};
    return cachedMap;
  }
}

function writeMap(data: AccountNoteMap) {
  cachedMap = data;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
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
