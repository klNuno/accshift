import {
  CLIENT_STORE_ACCOUNT_DEFAULT_GAME,
  getClientStoreRevision,
  getClientStoreValue,
  setClientStoreValue,
} from "$lib/storage/clientStorage";

export interface DefaultGame {
  appId: string;
  name: string;
}

type DefaultGameMap = Record<string, DefaultGame>;

let cachedMap: DefaultGameMap | null = null;
let cachedRevision = -1;

function sanitize(value: unknown): DefaultGameMap {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const out: DefaultGameMap = {};
  for (const [key, raw] of Object.entries(value as Record<string, unknown>)) {
    if (typeof key !== "string" || key.trim().length === 0) continue;
    if (!raw || typeof raw !== "object") continue;
    const entry = raw as Record<string, unknown>;
    const appId = typeof entry.appId === "string" ? entry.appId.trim() : "";
    const name = typeof entry.name === "string" ? entry.name : "";
    if (!appId || !/^\d+$/.test(appId)) continue;
    out[key] = { appId, name };
  }
  return out;
}

function readMap(): DefaultGameMap {
  const revision = getClientStoreRevision(CLIENT_STORE_ACCOUNT_DEFAULT_GAME);
  if (cachedMap && cachedRevision === revision) return cachedMap;
  const raw = getClientStoreValue<unknown>(CLIENT_STORE_ACCOUNT_DEFAULT_GAME);
  cachedMap = raw == null ? {} : sanitize(raw);
  cachedRevision = revision;
  return cachedMap;
}

function writeMap(data: DefaultGameMap) {
  cachedMap = data;
  setClientStoreValue(CLIENT_STORE_ACCOUNT_DEFAULT_GAME, data);
  cachedRevision = getClientStoreRevision(CLIENT_STORE_ACCOUNT_DEFAULT_GAME);
}

export function getDefaultGame(steamId: string): DefaultGame | null {
  return readMap()[steamId] ?? null;
}

export function setDefaultGame(steamId: string, game: DefaultGame) {
  const data = { ...readMap() };
  const appId = game.appId.trim();
  if (!appId || !/^\d+$/.test(appId)) return;
  data[steamId] = { appId, name: game.name };
  writeMap(data);
}

export function clearDefaultGame(steamId: string) {
  const data = { ...readMap() };
  if (!(steamId in data)) return;
  delete data[steamId];
  writeMap(data);
}

export function getDefaultGameRevision(): number {
  return getClientStoreRevision(CLIENT_STORE_ACCOUNT_DEFAULT_GAME);
}
