import type { Persona, PersonaAssignment } from "./types";
import { PLATFORM_DEFS } from "$lib/platforms/registry";
import {
  CLIENT_STORE_PERSONAS,
  getClientStoreValue,
  getClientStoreRevision,
  setClientStoreValue,
} from "$lib/storage/clientStorage";

export const PERSONA_COLORS = [
  "#f2a900",
  "#ef4444",
  "#3b82f6",
  "#22c55e",
  "#a855f7",
  "#ec4899",
  "#14b8a6",
  "#f97316",
] as const;

const DEFAULT_EMOJI = "🎭";
const NAME_MAX = 40;
const PLATFORM_IDS = new Set(PLATFORM_DEFS.map((platform) => platform.id));

let cache: Persona[] | null = null;
let cacheRevision = -1;

function sanitizeAssignments(value: unknown): PersonaAssignment[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const result: PersonaAssignment[] = [];
  for (const raw of value) {
    if (!raw || typeof raw !== "object") continue;
    const platformId = (raw as Record<string, unknown>).platformId;
    const accountId = (raw as Record<string, unknown>).accountId;
    if (typeof platformId !== "string" || typeof accountId !== "string") continue;
    if (!PLATFORM_IDS.has(platformId) || seen.has(platformId)) continue;
    if (!accountId) continue;
    seen.add(platformId);
    result.push({ platformId, accountId });
  }
  return result;
}

function sanitizePersona(value: unknown): Persona | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const id = typeof raw.id === "string" && raw.id ? raw.id : null;
  if (!id) return null;
  const name =
    typeof raw.name === "string" && raw.name.trim()
      ? raw.name.trim().slice(0, NAME_MAX)
      : "Persona";
  const emoji = typeof raw.emoji === "string" && raw.emoji ? raw.emoji.slice(0, 8) : DEFAULT_EMOJI;
  const color =
    typeof raw.color === "string" && /^#[0-9a-fA-F]{6}$/.test(raw.color)
      ? raw.color
      : PERSONA_COLORS[0];
  return { id, name, emoji, color, assignments: sanitizeAssignments(raw.assignments) };
}

function sanitizeList(value: unknown): Persona[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const result: Persona[] = [];
  for (const raw of value) {
    const persona = sanitizePersona(raw);
    if (!persona || seen.has(persona.id)) continue;
    seen.add(persona.id);
    result.push(persona);
  }
  return result;
}

export function getPersonas(): Persona[] {
  const revision = getClientStoreRevision(CLIENT_STORE_PERSONAS);
  if (!cache || cacheRevision !== revision) {
    cache = sanitizeList(getClientStoreValue(CLIENT_STORE_PERSONAS) ?? []);
    cacheRevision = revision;
  }
  return structuredClone(cache);
}

function persist(list: Persona[]) {
  const sanitized = sanitizeList(list);
  cache = sanitized;
  setClientStoreValue(CLIENT_STORE_PERSONAS, sanitized);
  cacheRevision = getClientStoreRevision(CLIENT_STORE_PERSONAS);
}

function newId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `persona-${getPersonas().length}-${performance.now()}`;
}

export function createPersona(input: Omit<Persona, "id">): Persona {
  const persona: Persona = { ...input, id: newId() };
  persist([...getPersonas(), persona]);
  return persona;
}

export function updatePersona(id: string, patch: Partial<Omit<Persona, "id">>) {
  persist(getPersonas().map((p) => (p.id === id ? { ...p, ...patch, id } : p)));
}

export function deletePersona(id: string) {
  persist(getPersonas().filter((p) => p.id !== id));
}
