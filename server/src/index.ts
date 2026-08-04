// Accshift telemetry Worker
// =========================
// Endpoints:
//   POST /track             - Mode A or B events (batch)
//   POST /consent           - one aggregate onboarding choice per installation
//   POST /forget            - GDPR art. 17, Mode B only
//   POST /export            - GDPR art. 20, Mode B only
//
// This Worker is a thin privacy proxy in front of PostHog. It stores nothing
// itself: no database, no object storage, no event log.
//
// Principles:
// - Mode A: a random local UUID is HMACed for unique-installation pings only.
//   Regular usage events keep a daily hash and remain unlinkable across days.
//   Nothing in Mode A creates a PostHog person profile.
// - Mode B: the install_id is the PostHog distinct_id, which is what makes
//   retention and per-installation deletion possible.
// - The client IP never reaches PostHog. It is processed in memory here to
//   derive `country` and the Mode A daily hash, then dropped. PostHog only ever
//   sees this Worker's address, and every event overrides $ip with a
//   placeholder and carries $geoip_disable so no location is inferred
//   downstream either.
// - The PostHog personal API key stays server-side. It is what /forget and
//   /export need, and it is exactly why the app talks to this Worker instead of
//   talking to PostHog directly: that key can read and delete the whole
//   project, so it can never ship inside a desktop binary.
// - CORS is restricted when browsers send Origin; native Tauri requests may omit it.
// - The app never uploads log files. Logs stay on the user's machine and are
//   shared only by hand, so this Worker has no object storage at all.

interface RateLimit {
  limit(options: { key: string }): Promise<{ success: boolean }>;
}

export interface Env {
  RL_TRACK: RateLimit;
  RL_RGPD: RateLimit;
  RL_GLOBAL: RateLimit;
  RL_NOTIFY: RateLimit;
  HASH_SECRET: string;
  POSTHOG_PROJECT_API_KEY: string;
  POSTHOG_PERSONAL_API_KEY: string;
  POSTHOG_PROJECT_ID: string;
  POSTHOG_INGEST_HOST: string;
  POSTHOG_API_HOST: string;
  RESEND_API_KEY: string;
  ALERT_EMAIL: string;
  ALERT_FROM: string;
  ENVIRONMENT: string;
  BATCH_MAX_EVENTS: string;
  ALLOWED_ORIGINS: string;
  UA_PREFIX: string;
}

// ─── Payload types ───────────────────────────────────────────────
export interface TelemetryEvent {
  name: string;
  app_version: string;
  os_version: string;
  // Fixed identifiers, so grouping by platform no longer means parsing
  // `os_version` prose on the dashboard side.
  os?: string;
  arch?: string;
  surface?: string;
  locale?: string;
  // When the event happened on the client, RFC 3339 UTC with second
  // resolution. A batch spans up to five minutes, so stamping every event
  // with its arrival time collapsed that window onto one point.
  client_ts?: string;
  platform?: string;
  duration_ms?: number;
  count?: number;
  success?: boolean;
  succeeded?: number;
  platforms?: number;
  dropped_events?: number;
  error_code?: string;
  operation?: string;
  target_version?: string;
  command?: string;
  ui_language?: string;
  enabled_platforms?: string[];
  personas_enabled?: boolean;
  pin_enabled?: boolean;
  cli_enabled?: boolean;
  deep_links_enabled?: boolean;
  streamer_mode?: string;
  animations?: string;
}

interface TrackPayload {
  mode: "A" | "B";
  install_id?: string;
  anonymous_id?: string;
  events: TelemetryEvent[];
}

interface ConsentPayload {
  choice?: "refused" | "basic" | "enhanced";
  app_version?: string;
}

// ─── Entry point ─────────────────────────────────────────────────
export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    if (request.method === "OPTIONS")
      return cors(new Response(null, { status: 204 }), request, env);

    const url = new URL(request.url);
    try {
      let response: Response;
      switch (url.pathname) {
        case "/":
          response = json({ ok: true, service: "accshift-telemetry" });
          break;
        case "/track":
          response =
            request.method === "POST" ? await handleTrack(request, env, ctx) : methodNotAllowed();
          break;
        case "/consent":
          response =
            request.method === "POST" ? await handleConsent(request, env, ctx) : methodNotAllowed();
          break;
        case "/forget":
          response =
            request.method === "POST" ? await handleForget(request, env, ctx) : methodNotAllowed();
          break;
        case "/export":
          response =
            request.method === "POST" ? await handleExport(request, env, ctx) : methodNotAllowed();
          break;
        default:
          response = json({ error: "not_found" }, 404);
      }
      return cors(response, request, env);
    } catch (err) {
      console.error("unhandled", err);
      return cors(json({ error: "internal_error" }, 500), request, env);
    }
  },
};

// ─── /track ──────────────────────────────────────────────────────

const TRACK_BODY_MAX_BYTES = 64 * 1024;

async function handleTrack(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_TRACK, ip, "/track", ctx, true);
  if (blocked) return blocked;

  // Parse and validate the body BEFORE charging the global limiter, so
  // malformed or empty requests cannot eat into the shared quota. The UA check
  // and per-IP limit above already gate cheaply on the headers / IP.
  const parsed = await readJsonCapped<TrackPayload>(request, TRACK_BODY_MAX_BYTES);
  if (parsed instanceof Response) return parsed;
  const payload = parsed;
  if (!payload || !Array.isArray(payload.events) || payload.events.length === 0) {
    return json({ error: "bad_payload" }, 400);
  }

  const maxEvents = intVar(env.BATCH_MAX_EVENTS, 200);
  if (payload.events.length > maxEvents) {
    return json({ error: "batch_too_large", max: maxEvents }, 413);
  }

  // A valid event carries a non-empty string name; reject a batch with none.
  const usable = payload.events.filter((e) => typeof e?.name === "string" && e.name !== "");
  if (usable.length === 0) {
    return json({ error: "bad_payload" }, 400);
  }

  if (payload.mode !== "A" && payload.mode !== "B") {
    return json({ error: "bad_mode" }, 400);
  }
  if (payload.mode === "B" && !isUuidV4(payload.install_id)) {
    return json({ error: "bad_install_id" }, 400);
  }
  if (
    payload.mode === "A" &&
    payload.anonymous_id !== undefined &&
    !isUuidV4(payload.anonymous_id)
  ) {
    return json({ error: "bad_anonymous_id" }, 400);
  }

  const overGlobal = await enforceGlobalLimit(env, "/track", ctx);
  if (overGlobal) return overGlobal;

  const country = (request.cf?.country as string | undefined) ?? "XX";
  const nowIso = new Date().toISOString();
  const todayIso = nowIso.slice(0, 10);

  // Identifier selection is the whole privacy model, so it lives in one place.
  //
  // Mode B: the install_id identifies every event, which is the point of the
  // mode and what makes retention and art. 17 deletion possible.
  //
  // Mode A: usage events get a hash that rotates daily, so they cannot be
  // linked across days. `ping` is the one exception: it gets a stable hash
  // derived from the local random UUID, because counting unique installations
  // is impossible with an identifier that changes every night. That stable
  // hash is purpose-bound (it is HMACed with "basic-ping") and never touches a
  // person profile.
  let eventIdentifier: string;
  let pingIdentifier: string;
  if (payload.mode === "B") {
    eventIdentifier = payload.install_id!;
    pingIdentifier = eventIdentifier;
  } else {
    const ua = request.headers.get("User-Agent") ?? "";
    eventIdentifier = await dailyVisitorHash(ip, ua, todayIso, env.HASH_SECRET);
    pingIdentifier = payload.anonymous_id
      ? await stableAnonymousHash(payload.anonymous_id, "basic-ping", env.HASH_SECRET)
      : eventIdentifier;
  }

  const batch = buildBatch(
    payload.mode,
    usable,
    { eventIdentifier, pingIdentifier },
    country,
    nowIso,
  );

  const sent = await posthogCapture(env, batch);
  if (!sent.ok) {
    console.error("posthog capture failed", sent.status, sent.body);
    return json({ error: "upstream_unavailable" }, 502);
  }

  return json({ ok: true, accepted: batch.length });
}

// ─── /consent ────────────────────────────────────────────────────

const CONSENT_BODY_MAX_BYTES = 4 * 1024;
const CONSENT_CHOICES = new Set(["refused", "basic", "enhanced"]);

// Every onboarding choice lands on this single distinct_id. It is a counter,
// not a person: the whole point of the endpoint is that a choice, including a
// refusal, carries nothing that ties it back to an installation.
const CONSENT_DISTINCT_ID = "consent-aggregate";

async function handleConsent(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_TRACK, ip, "/consent", ctx, true);
  if (blocked) return blocked;

  const parsed = await readJsonCapped<ConsentPayload>(request, CONSENT_BODY_MAX_BYTES);
  if (parsed instanceof Response) return parsed;
  if (!parsed || typeof parsed.choice !== "string" || !CONSENT_CHOICES.has(parsed.choice)) {
    return json({ error: "bad_payload" }, 400);
  }

  const overGlobal = await enforceGlobalLimit(env, "/consent", ctx);
  if (overGlobal) return overGlobal;

  // No country here, unlike /track. A refusal must stay a pure counter.
  const sent = await posthogCapture(env, [
    {
      event: "consent_choice",
      distinct_id: CONSENT_DISTINCT_ID,
      timestamp: new Date().toISOString(),
      properties: {
        distinct_id: CONSENT_DISTINCT_ID,
        $process_person_profile: false,
        ...privacyProperties(),
        choice: parsed.choice,
        app_version: parsed.app_version ?? "",
      },
    },
  ]);
  if (!sent.ok) {
    console.error("posthog consent failed", sent.status, sent.body);
    return json({ error: "upstream_unavailable" }, 502);
  }

  return json({ ok: true });
}

// ─── /forget (Mode B, GDPR art. 17) ──────────────────────────────

const RGPD_BODY_MAX_BYTES = 4 * 1024;

async function handleForget(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_RGPD, ip, "/forget", ctx, true);
  if (blocked) return blocked;

  const parsed = await readJsonCapped<{ install_id?: string }>(request, RGPD_BODY_MAX_BYTES);
  if (parsed instanceof Response) return parsed;
  const payload = parsed;
  if (!payload || !isUuidV4(payload.install_id)) return json({ error: "bad_install_id" }, 400);

  const overGlobal = await enforceGlobalLimit(env, "/forget", ctx);
  if (overGlobal) return overGlobal;

  const id = payload.install_id!;

  const person = await posthogFindPerson(env, id);
  if (person.error) {
    console.error("posthog person lookup failed", redactUuids(person.error));
    return json({ error: "upstream_unavailable" }, 502);
  }
  // Nothing to delete. Answering ok is what lets the client clear the
  // identifier from its local deletion outbox instead of retrying forever.
  if (person.personId === null) {
    return json({ ok: true, deleted: false, reason: "not_found" });
  }

  const deleted = await posthogDeletePerson(env, person.personId);
  if (!deleted.ok) {
    console.error("posthog person delete failed", deleted.status, redactUuids(deleted.body));
    return json({ error: "upstream_unavailable" }, 502);
  }

  return json({ ok: true, deleted: true });
}

// ─── /export (Mode B, GDPR art. 20) ──────────────────────────────

const EXPORT_MAX_ROWS = 10000;

async function handleExport(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_RGPD, ip, "/export", ctx, true);
  if (blocked) return blocked;

  const parsed = await readJsonCapped<{ install_id?: string }>(request, RGPD_BODY_MAX_BYTES);
  if (parsed instanceof Response) return parsed;
  const payload = parsed;
  if (!payload || !isUuidV4(payload.install_id)) return json({ error: "bad_install_id" }, 400);
  const id = payload.install_id!;

  const overGlobal = await enforceGlobalLimit(env, "/export", ctx);
  if (overGlobal) return overGlobal;

  // `id` passed isUuidV4, so it is hex and dashes only and cannot carry a
  // quote. That is what makes the interpolation below safe.
  const query = [
    "SELECT timestamp, event, properties",
    "FROM events",
    `WHERE distinct_id = '${id}'`,
    "ORDER BY timestamp DESC",
    `LIMIT ${EXPORT_MAX_ROWS}`,
  ].join(" ");

  const [events, person] = await Promise.all([
    posthogQuery(env, query, "gdpr-export"),
    posthogFindPerson(env, id),
  ]);

  if (events.error) {
    // Redacted: a HogQL error echoes the failing query, which carries the
    // install_id. That is pseudonymous personal data and it has no business
    // sitting in the Worker's log retention just because a query failed.
    console.error("posthog export query failed", redactUuids(events.error));
    return json({ error: "upstream_unavailable" }, 502);
  }

  const rows = (events.results ?? []).map((row) => ({
    timestamp: row[0] ?? null,
    event: row[1] ?? null,
    properties: row[2] ?? null,
  }));

  return json({
    install_id: id,
    exported_at: new Date().toISOString(),
    person_properties: person.properties ?? null,
    events: rows,
    truncated: rows.length >= EXPORT_MAX_ROWS,
    note: "Mode A events are excluded by design: they carry no install_id and cannot be attributed to an installation.",
  });
}

// ─── PostHog ─────────────────────────────────────────────────────

// Ingestion path on the PostHog ingest host (eu.i.posthog.com by default).
const POSTHOG_BATCH_PATH = "/batch/";
const POSTHOG_TIMEOUT_MS = 10000;

export interface PostHogBatchItem {
  event: string;
  distinct_id: string;
  timestamp: string;
  properties: Record<string, unknown>;
}

// The address sent in place of a real one. Must stay TRUTHY.
//
// `null` looks like the obvious choice and is a trap: PostHog's ingestion
// back-fills the property from the request socket whenever the value is falsy
// (`if (!properties['$ip'] && event.ip) properties['$ip'] = event.ip`). Sending
// null therefore stores an address rather than suppressing one. Only a truthy
// value wins, so we send an unroutable placeholder.
const IP_PLACEHOLDER = "0.0.0.0";

// Properties attached to every event we forward, without exception.
//
// The address PostHog would otherwise record is this Worker's egress address,
// never the user's, since the user's connection terminates at Cloudflare. The
// placeholder makes that independent of PostHog behaviour instead of a
// consequence of the topology. Turning on "Discard client IP data" on the
// PostHog project drops the property altogether and is the belt to this
// suspenders; see the README.
//
// $geoip_disable stops the GeoIP enrichment that would otherwise run and stamp
// every event with a location inferred from a Cloudflare datacenter. It is an
// early return in PostHog's transformation, so no $geoip_* property is written
// at all. The real country is derived here from request.cf and sent as a plain
// property.
function privacyProperties(): Record<string, unknown> {
  return { $ip: IP_PLACEHOLDER, $geoip_disable: true };
}

export interface BatchIdentifiers {
  /// Identifies every event except `ping`.
  eventIdentifier: string;
  /// Identifies `ping` only. Same value as eventIdentifier in Mode B.
  pingIdentifier: string;
}

// ─── Property validation ─────────────────────────────────────────
//
// Everything below runs on a payload a modified client could have written, so
// each value is checked for shape before it is forwarded. The app already
// maps these fields onto closed vocabularies; this is the half of that
// guarantee that does not depend on the client being the one we shipped.

const CODE_RE = /^[a-z0-9_]{1,40}$/;
const PLATFORM_RE = /^[a-z0-9_-]{1,32}$/;
const VERSION_RE = /^[A-Za-z0-9.+-]{1,32}$/;
const CLIENT_TS_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/;

const MAX_ENABLED_PLATFORMS = 16;

/// How far a client timestamp may sit from the server's clock.
///
/// A wrong client clock is common (a machine fresh out of a long sleep, a
/// dual-boot with local-time RTC). Beyond a day the value stops being a
/// better estimate than arrival time and starts dragging events into weeks
/// that never happened, so it is dropped rather than trusted.
const MAX_CLOCK_SKEW_MS = 24 * 60 * 60 * 1000;

function code(value: unknown): string | undefined {
  return typeof value === "string" && CODE_RE.test(value) ? value : undefined;
}

function platformId(value: unknown): string | undefined {
  return typeof value === "string" && PLATFORM_RE.test(value) ? value : undefined;
}

function version(value: unknown): string | undefined {
  return typeof value === "string" && VERSION_RE.test(value) ? value : undefined;
}

function count(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : undefined;
}

function flag(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}

function platformList(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const ids = value.map(platformId).filter((id): id is string => id !== undefined);
  return ids.length === 0 ? undefined : ids.slice(0, MAX_ENABLED_PLATFORMS);
}

// Picks the timestamp an event is stored under: the client's own, when it is
// well-formed and plausible, otherwise the moment the batch arrived.
export function eventTimestamp(clientTs: unknown, serverIso: string): string {
  if (typeof clientTs !== "string" || !CLIENT_TS_RE.test(clientTs)) return serverIso;
  const parsed = Date.parse(clientTs);
  const server = Date.parse(serverIso);
  if (!Number.isFinite(parsed) || !Number.isFinite(server)) return serverIso;
  return Math.abs(parsed - server) > MAX_CLOCK_SKEW_MS ? serverIso : clientTs;
}

// Turns a validated /track payload into PostHog batch items.
//
// Pure on purpose: this is where the privacy model becomes concrete bytes, so
// it has to be assertable in a test without a network, a clock or a secret.
// Every property that reaches PostHog is written here and nowhere else.
export function buildBatch(
  mode: "A" | "B",
  events: TelemetryEvent[],
  ids: BatchIdentifiers,
  country: string,
  timestamp: string,
): PostHogBatchItem[] {
  const isModeB = mode === "B";
  return events.map((ev) => {
    const distinctId = ev.name === "ping" ? ids.pingIdentifier : ids.eventIdentifier;
    const properties: Record<string, unknown> = {
      distinct_id: distinctId,
      // Mode A never materializes a person. Unique-installation counts still
      // work: PostHog counts distinct_id on the events themselves.
      $process_person_profile: isModeB,
      ...privacyProperties(),
      telemetry_mode: mode,
      country,
      app_version: ev.app_version ?? "",
      os_version: ev.os_version ?? "",
    };
    // Optional fields are omitted rather than sent empty, so an absent value
    // stays distinguishable from an empty one on the dashboard side. Each one
    // is validated: this function is the only place a property can be written,
    // so anything not listed here cannot reach PostHog at all.
    const optional: Array<[string, unknown]> = [
      ["os", code(ev.os)],
      ["arch", code(ev.arch)],
      ["surface", code(ev.surface)],
      ["locale", typeof ev.locale === "string" && ev.locale.length <= 35 ? ev.locale : undefined],
      ["platform", platformId(ev.platform)],
      ["duration_ms", count(ev.duration_ms)],
      ["count", count(ev.count)],
      ["success", flag(ev.success)],
      ["succeeded", count(ev.succeeded)],
      ["platforms", count(ev.platforms)],
      ["dropped_events", count(ev.dropped_events)],
      ["error_code", code(ev.error_code)],
      ["operation", code(ev.operation)],
      ["target_version", version(ev.target_version)],
      ["command", code(ev.command)],
      ["ui_language", code(ev.ui_language)],
      ["enabled_platforms", platformList(ev.enabled_platforms)],
      ["personas_enabled", flag(ev.personas_enabled)],
      ["pin_enabled", flag(ev.pin_enabled)],
      ["cli_enabled", flag(ev.cli_enabled)],
      ["deep_links_enabled", flag(ev.deep_links_enabled)],
      ["streamer_mode", code(ev.streamer_mode)],
      ["animations", code(ev.animations)],
    ];
    for (const [key, value] of optional) {
      if (value !== undefined) properties[key] = value;
    }
    // Person properties are Mode B only, by construction: Mode A has no person
    // to attach them to.
    if (isModeB) {
      properties.$set = {
        app_version: ev.app_version ?? "",
        os_version: ev.os_version ?? "",
        country,
        ...(ev.locale ? { locale: ev.locale } : {}),
      };
    }
    return {
      event: ev.name,
      distinct_id: distinctId,
      timestamp: eventTimestamp(ev.client_ts, timestamp),
      properties,
    };
  });
}

async function posthogCapture(
  env: Env,
  batch: PostHogBatchItem[],
): Promise<{ ok: boolean; status: number; body: string }> {
  if (batch.length === 0) return { ok: true, status: 200, body: "" };
  const url = `${trimHost(env.POSTHOG_INGEST_HOST)}${POSTHOG_BATCH_PATH}`;
  try {
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ api_key: env.POSTHOG_PROJECT_API_KEY, batch }),
      signal: AbortSignal.timeout(POSTHOG_TIMEOUT_MS),
    });
    const body = res.ok ? "" : await res.text().catch(() => "");
    return { ok: res.ok, status: res.status, body: body.slice(0, 500) };
  } catch (e) {
    return { ok: false, status: 0, body: String(e) };
  }
}

function posthogApiHeaders(env: Env): HeadersInit {
  return {
    Authorization: `Bearer ${env.POSTHOG_PERSONAL_API_KEY}`,
    "Content-Type": "application/json",
  };
}

function posthogProjectBase(env: Env): string {
  return `${trimHost(env.POSTHOG_API_HOST)}/api/projects/${env.POSTHOG_PROJECT_ID}`;
}

interface PersonLookup {
  personId: string | null;
  properties?: Record<string, unknown> | null;
  error?: string;
}

// Resolves a distinct_id to a PostHog person. personId is null when the person
// does not exist, which is a normal outcome: an installation that opted out
// before ever sending an event has nothing on the server.
async function posthogFindPerson(env: Env, distinctId: string): Promise<PersonLookup> {
  const url = `${posthogProjectBase(env)}/persons/?distinct_id=${encodeURIComponent(distinctId)}`;
  try {
    const res = await fetch(url, {
      method: "GET",
      headers: posthogApiHeaders(env),
      signal: AbortSignal.timeout(POSTHOG_TIMEOUT_MS),
    });
    if (!res.ok) {
      return { personId: null, error: `status ${res.status}` };
    }
    const data = (await res.json()) as {
      results?: Array<{
        id?: number | string;
        uuid?: string;
        properties?: Record<string, unknown>;
      }>;
    };
    const first = data.results?.[0];
    if (!first) return { personId: null };
    // The delete endpoint accepts either the person UUID or the integer id.
    // PostHog documents the UUID, so prefer it and keep `id` as the fallback.
    const personId = first.uuid ?? (first.id === undefined || first.id === null ? null : first.id);
    if (personId === null) return { personId: null };
    return { personId: String(personId), properties: first.properties ?? null };
  } catch (e) {
    return { personId: null, error: String(e) };
  }
}

// delete_events is the part that matters for art. 17: without it the person
// record goes but their events stay. PostHog switches on the mere PRESENCE of
// the parameter, not its value, so `delete_events=false` would also delete.
//
// The call returns 202, not 200: the person row goes immediately, the events
// are queued for an asynchronous batch job. That job only sweeps events
// captured BEFORE the request, so anything still in flight when it runs
// survives. In practice the client disables Mode B and refreshes its consent
// state before calling /forget, which closes the queue first; the residual
// window is a batch already on the wire.
async function posthogDeletePerson(
  env: Env,
  personId: string,
): Promise<{ ok: boolean; status: number; body: string }> {
  const url = `${posthogProjectBase(env)}/persons/${encodeURIComponent(personId)}/?delete_events=true`;
  try {
    const res = await fetch(url, {
      method: "DELETE",
      headers: posthogApiHeaders(env),
      signal: AbortSignal.timeout(POSTHOG_TIMEOUT_MS),
    });
    const body = res.ok ? "" : await res.text().catch(() => "");
    return { ok: res.ok, status: res.status, body: body.slice(0, 500) };
  } catch (e) {
    return { ok: false, status: 0, body: String(e) };
  }
}

interface QueryResult {
  results?: unknown[][];
  error?: string;
}

async function posthogQuery(env: Env, query: string, name: string): Promise<QueryResult> {
  const url = `${posthogProjectBase(env)}/query/`;
  try {
    const res = await fetch(url, {
      method: "POST",
      headers: posthogApiHeaders(env),
      body: JSON.stringify({ query: { kind: "HogQLQuery", query }, name }),
      signal: AbortSignal.timeout(POSTHOG_TIMEOUT_MS),
    });
    if (!res.ok) {
      const body = await res.text().catch(() => "");
      return { error: `status ${res.status}: ${body.slice(0, 300)}` };
    }
    const data = (await res.json()) as { results?: unknown[][] };
    return { results: data.results ?? [] };
  } catch (e) {
    return { error: String(e) };
  }
}

function trimHost(host: string): string {
  return (host || "").replace(/\/+$/, "");
}

// ─── Utils ───────────────────────────────────────────────────────

async function dailyVisitorHash(
  ip: string,
  ua: string,
  date: string,
  secret: string,
): Promise<string> {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(`${date}|${ip}|${ua}`));
  return bytesToHex(new Uint8Array(sig)).slice(0, 32);
}

async function stableAnonymousHash(
  anonymousId: string,
  purpose: string,
  secret: string,
): Promise<string> {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(`${purpose}|${anonymousId}`));
  return bytesToHex(new Uint8Array(sig)).slice(0, 32);
}

function bytesToHex(bytes: Uint8Array): string {
  const HEX = "0123456789abcdef";
  let s = "";
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i]!;
    s += HEX[b >> 4]! + HEX[b & 0x0f]!;
  }
  return s;
}

// Replaces any UUID in a string with a fixed marker. Used before logging an
// upstream error message, which may quote back a request that carried one.
export function redactUuids(s: string): string {
  return s.replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, "<uuid>");
}

function isUuidV4(s: unknown): s is string {
  return (
    typeof s === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(s)
  );
}

// Parses a JSON body with a byte cap. The stream is stopped as soon as the cap
// is crossed, so chunked requests cannot force an unbounded allocation.
export async function readJsonCapped<T>(
  request: Request,
  maxBytes: number,
): Promise<T | Response | null> {
  const lenHeader = request.headers.get("Content-Length");
  if (lenHeader && parseInt(lenHeader, 10) > maxBytes) {
    return json({ error: "payload_too_large", max: maxBytes }, 413);
  }

  const reader = request.body?.getReader();
  if (!reader) return null;
  const decoder = new TextDecoder();
  let totalBytes = 0;
  let text = "";
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      totalBytes += value.byteLength;
      if (totalBytes > maxBytes) {
        await reader.cancel();
        return json({ error: "payload_too_large", max: maxBytes }, 413);
      }
      text += decoder.decode(value, { stream: true });
    }
    text += decoder.decode();
  } catch {
    return null;
  } finally {
    reader.releaseLock();
  }

  try {
    return JSON.parse(text) as T;
  } catch {
    return null;
  }
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json; charset=utf-8" },
  });
}

function methodNotAllowed(): Response {
  return json({ error: "method_not_allowed" }, 405);
}

function allowedOrigins(env: Env): Set<string> {
  return new Set(
    (env.ALLOWED_ORIGINS || "")
      .split(",")
      .map((origin) => origin.trim())
      .filter(Boolean),
  );
}

function cors(res: Response, request: Request, env: Env): Response {
  const h = new Headers(res.headers);
  const origin = request.headers.get("Origin");
  const allowed = allowedOrigins(env);
  if (origin && allowed.has(origin)) {
    h.set("Access-Control-Allow-Origin", origin);
    h.set("Vary", "Origin");
  } else if (!origin) {
    h.set("Access-Control-Allow-Origin", "null");
  }
  h.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
  h.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
  h.set("Access-Control-Max-Age", "86400");
  return new Response(res.body, { status: res.status, headers: h });
}

// ─── Anti-abuse ──────────────────────────────────────────────────

function checkUa(request: Request, env: Env): Response | null {
  const prefix = env.UA_PREFIX || "Accshift/";
  const ua = request.headers.get("User-Agent") ?? "";
  if (!ua.startsWith(prefix)) return json({ error: "bad_ua" }, 400);
  return null;
}

function intVar(v: string | undefined, fallback: number): number {
  const n = parseInt(v ?? "", 10);
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

// Throughput ceiling on a shared key, on top of the per-IP limiters.
//
// This replaces the daily budget counters the Worker used to keep in D1. The
// cost being defended has moved: Cloudflare no longer stores anything, so what
// a distributed flood can now burn is the PostHog monthly event quota, and
// burning it would take real data down with it. A limiter keyed on a constant
// bounds that with no storage at all.
//
// It is NOT an account-wide counter, despite the key. Cloudflare documents one
// limit per key PER LOCATION, cached on the machine running the Worker and
// updated asynchronously: "The Rate Limiting API is permissive, eventually
// consistent, and intentionally designed to not be used as an accurate
// accounting system." A flood spread across colos therefore multiplies the
// effective ceiling by the number of locations it reaches. This bounds a
// single-origin burst and nothing more. The hard spend ceiling lives in the
// PostHog project billing limit, which is the only place that can actually
// stop a charge.
async function enforceGlobalLimit(
  env: Env,
  endpoint: string,
  ctx: ExecutionContext,
): Promise<Response | null> {
  const { success } = await env.RL_GLOBAL.limit({ key: "global" });
  if (success) return null;
  ctx.waitUntil(notifyRateLimit(env, `${endpoint} (global)`, ""));
  return json({ error: "global_rate_limited", endpoint }, 503);
}

function clientIp(request: Request): string {
  return request.headers.get("CF-Connecting-IP") ?? "";
}

// Mask an IP keeping only the /24 (v4) or /48 (v6) prefix so an alert email
// gives a coarse geographic hint without being a full PII.
export function maskIp(ip: string): string {
  if (!ip) return "unknown";
  if (ip.includes(":")) {
    // Only the groups before a "::" run are the real prefix. Splitting on ":"
    // alone turns 2001:db8::1 into the malformed "2001:db8:::/48", and keeping
    // the tail would put a trailing group in a prefix position.
    const head = (ip.split("::")[0] ?? "").split(":").filter(Boolean).slice(0, 3);
    while (head.length < 3) head.push("0");
    return head.join(":") + "::/48";
  }
  const parts = ip.split(".");
  if (parts.length === 4) return `${parts[0]}.${parts[1]}.${parts[2]}.x`;
  return "unknown";
}

async function enforceRateLimit(
  env: Env,
  binding: RateLimit,
  ip: string,
  endpoint: string,
  ctx: ExecutionContext,
  requireIp = false,
): Promise<Response | null> {
  if (!ip) {
    // No trustworthy IP (e.g. local dev). Public write endpoints fail closed.
    return requireIp ? json({ error: "no_client_ip" }, 400) : null;
  }
  const { success } = await binding.limit({ key: ip });
  if (success) return null;
  ctx.waitUntil(notifyRateLimit(env, endpoint, ip));
  return json({ error: "rate_limited", endpoint }, 429);
}

async function notifyRateLimit(env: Env, endpoint: string, ip: string): Promise<void> {
  // Throttle: at most 1 email per minute per (endpoint, IP) pair.
  const throttleKey = `${endpoint}:${ip}`;
  const { success } = await env.RL_NOTIFY.limit({ key: throttleKey });
  if (!success) return;

  if (!env.RESEND_API_KEY || !env.ALERT_EMAIL) return;

  const subject = `[Accshift telemetry] rate limit hit on ${endpoint}`;
  const text = [
    `Endpoint: ${endpoint}`,
    `Masked IP: ${maskIp(ip)}`,
    `When: ${new Date().toISOString()}`,
    ``,
    `The Worker returned 429 for this IP on this endpoint.`,
    `Configured limit reached. No immediate action required:`,
    `excess requests are blocked at the edge, no cost.`,
    ``,
    `Subsequent emails for this (endpoint, IP) pair are throttled to 1/minute.`,
  ].join("\n");

  try {
    const res = await fetch("https://api.resend.com/emails", {
      method: "POST",
      headers: {
        Authorization: `Bearer ${env.RESEND_API_KEY}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        from: env.ALERT_FROM,
        to: [env.ALERT_EMAIL],
        subject,
        text,
      }),
    });
    if (!res.ok) console.error("resend status", res.status, await res.text());
  } catch (e) {
    console.error("resend error", e);
  }
}
