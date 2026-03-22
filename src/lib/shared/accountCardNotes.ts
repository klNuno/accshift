import { CLIENT_STORE_ACCOUNT_CARD_NOTES } from "$lib/storage/clientStorage";
import { createCachedMapStore } from "./cachedMapStore";

const MAX_NOTE_LENGTH = 180;

function sanitizeNote(_key: string, value: string): string | null {
  const clean = value
    .replace(/\p{Cc}/gu, " ")
    .trim()
    .slice(0, MAX_NOTE_LENGTH);
  return clean || null;
}

const store = createCachedMapStore(CLIENT_STORE_ACCOUNT_CARD_NOTES, sanitizeNote);

export const getAccountCardNote = store.get;
export const setAccountCardNote = store.set;
export const clearAccountCardNote = store.remove;
