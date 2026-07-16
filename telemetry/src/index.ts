// Accshift telemetry Worker
// =========================
// Endpoints:
//   POST /track             - Mode A or B events (batch)
//   POST /consent           - one aggregate onboarding choice per installation
//   POST /forget            - GDPR art. 17, Mode B only
//   POST /export            - GDPR art. 20, Mode B only
//   POST /admin/query       - read-only D1 SELECT for an external dashboard
//
// Principles:
// - Mode A: a random local UUID is HMACed for unique-installation pings only.
//   Regular usage events keep a daily hash and remain unlinkable across days.
// - IP is never stored. Country is derived from request.cf.country.
// - CORS is restricted when browsers send Origin; native Tauri requests may omit it.
// - The app never uploads log files. Logs stay on the user's machine and are
//   shared only by hand, so this Worker has no object storage at all.

interface RateLimit {
  limit(options: { key: string }): Promise<{ success: boolean }>;
}

export interface Env {
  AE: AnalyticsEngineDataset;
  DB: D1Database;
  RL_TRACK: RateLimit;
  RL_RGPD: RateLimit;
  RL_ADMIN: RateLimit;
  RL_NOTIFY: RateLimit;
  HASH_SECRET: string;
  ADMIN_TOKEN: string;
  RESEND_API_KEY: string;
  ALERT_EMAIL: string;
  ALERT_FROM: string;
  ENVIRONMENT: string;
  BATCH_MAX_EVENTS: string;
  ADMIN_ALLOWED: string;
  ALLOWED_ORIGINS: string;
  UA_PREFIX: string;
  BUDGET_TRACK: string;
  BUDGET_CONSENT: string;
  BUDGET_FORGET: string;
  BUDGET_EXPORT: string;
  BUDGET_ADMIN: string;
}

// ─── Payload types ───────────────────────────────────────────────
interface TelemetryEvent {
  name: string;
  app_version: string;
  os_version: string;
  locale?: string;
  platform?: string;
  duration_ms?: number;
  count?: number;
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
        case "/admin/query":
          response =
            request.method === "POST"
              ? await handleAdminQuery(request, env, ctx)
              : methodNotAllowed();
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

  // Parse and validate the body BEFORE touching the daily budget, so malformed
  // or empty requests cannot burn the D1 budget counter. UA check and rate
  // limit above already gate cheaply on the headers / IP.
  const parsed = await readJsonCapped<TrackPayload>(request, TRACK_BODY_MAX_BYTES);
  if (parsed instanceof Response) return parsed;
  const payload = parsed;
  if (!payload || !Array.isArray(payload.events) || payload.events.length === 0) {
    return json({ error: "bad_payload" }, 400);
  }

  const maxEvents = parseInt(env.BATCH_MAX_EVENTS, 10) || 200;
  if (payload.events.length > maxEvents) {
    return json({ error: "batch_too_large", max: maxEvents }, 413);
  }

  // A valid event carries a non-empty string name; reject a batch with none.
  const hasValidEvent = payload.events.some((e) => typeof e?.name === "string" && e.name !== "");
  if (!hasValidEvent) {
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

  // Payload is well-formed and carries at least one usable event: now charge
  // the daily budget.
  const budgetErr = await checkBudget(env, "/track", intVar(env.BUDGET_TRACK, 4000), ctx);
  if (budgetErr) return budgetErr;

  const country = (request.cf?.country as string | undefined) ?? "XX";
  const todayIso = new Date().toISOString().slice(0, 10);

  let eventIdentifier: string;
  let pingIdentifier: string;
  let isPersistent: 0 | 1;
  if (payload.mode === "B") {
    eventIdentifier = payload.install_id!;
    pingIdentifier = eventIdentifier;
    isPersistent = 1;
    // GDPR: drop the batch if the install_id is in the forget list.
    const forgotten = await env.DB.prepare("SELECT 1 FROM forgotten WHERE install_id = ?1")
      .bind(eventIdentifier)
      .first();
    if (forgotten) return json({ ok: true, forgotten: true });
  } else {
    const ua = request.headers.get("User-Agent") ?? "";
    eventIdentifier = await dailyVisitorHash(ip, ua, todayIso, env.HASH_SECRET);
    pingIdentifier = payload.anonymous_id
      ? await stableAnonymousHash(payload.anonymous_id, "basic-ping", env.HASH_SECRET)
      : eventIdentifier;
    isPersistent = 0;
  }

  // Analytics Engine: one datapoint per event.
  for (const ev of payload.events) {
    if (!ev.name || typeof ev.name !== "string") continue;
    env.AE.writeDataPoint({
      indexes: [eventIdentifier.slice(0, 96)],
      blobs: [
        ev.name,
        ev.app_version ?? "",
        ev.os_version ?? "",
        country,
        ev.platform ?? "",
        ev.locale ?? "",
      ],
      doubles: [
        typeof ev.duration_ms === "number" ? ev.duration_ms : 0,
        typeof ev.count === "number" ? ev.count : 0,
      ],
    });
  }

  // D1: upsert daily_pings if a ping event is in the batch.
  const pingEvent = payload.events.find((e) => e.name === "ping");
  if (pingEvent) {
    await env.DB.prepare(
      `INSERT INTO daily_pings (identifier, is_persistent, date, app_version, os_version, locale, country)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(identifier, date) DO UPDATE SET
           app_version = excluded.app_version,
           os_version  = excluded.os_version,
           locale      = excluded.locale,
           country     = excluded.country`,
    )
      .bind(
        pingIdentifier,
        isPersistent,
        todayIso,
        pingEvent.app_version ?? "",
        pingEvent.os_version ?? "",
        pingEvent.locale ?? null,
        country,
      )
      .run();
  }

  // D1: accounts_snapshot, Mode B only (needs a stable install_id).
  if (isPersistent) {
    const snapshots = payload.events.filter((e) => e.name === "accounts_snapshot" && e.platform);
    if (snapshots.length > 0) {
      const stmts = snapshots.map((s) =>
        env.DB.prepare(
          `INSERT INTO accounts_snapshot (install_id, date, platform, count)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(install_id, date, platform) DO UPDATE SET count = excluded.count`,
        ).bind(eventIdentifier, todayIso, s.platform!, Math.max(0, Math.floor(s.count ?? 0))),
      );
      await env.DB.batch(stmts);
    }
  }

  return json({ ok: true, accepted: payload.events.length });
}

// ─── /consent ────────────────────────────────────────────────────

const CONSENT_BODY_MAX_BYTES = 4 * 1024;
const CONSENT_CHOICES = new Set(["refused", "basic", "enhanced"]);

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

  const budgetErr = await checkBudget(env, "/consent", intVar(env.BUDGET_CONSENT, 4000), ctx);
  if (budgetErr) return budgetErr;

  const today = new Date().toISOString().slice(0, 10);
  await env.DB.prepare(
    `INSERT INTO consent_choice_counts (date, app_version, choice, count)
       VALUES (?1, ?2, ?3, 1)
       ON CONFLICT(date, app_version, choice) DO UPDATE SET count = count + 1`,
  )
    .bind(today, parsed.app_version ?? "", parsed.choice)
    .run();

  return json({ ok: true });
}

// ─── /forget (Mode B) ────────────────────────────────────────────

const RGPD_BODY_MAX_BYTES = 4 * 1024;

async function handleForget(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_RGPD, ip, "/forget", ctx, true);
  if (blocked) return blocked;

  // Parse and validate the body BEFORE touching the daily budget, so malformed
  // or empty requests cannot burn the D1 budget counter. Mirrors handleTrack.
  const parsed = await readJsonCapped<{ install_id?: string }>(request, RGPD_BODY_MAX_BYTES);
  if (parsed instanceof Response) return parsed;
  const payload = parsed;
  if (!payload || !isUuidV4(payload.install_id)) return json({ error: "bad_install_id" }, 400);

  const budgetErr = await checkBudget(env, "/forget", intVar(env.BUDGET_FORGET, 500), ctx);
  if (budgetErr) return budgetErr;

  const id = payload.install_id!;
  const now = Math.floor(Date.now() / 1000);

  await env.DB.batch([
    env.DB.prepare("DELETE FROM daily_pings WHERE identifier = ?1 AND is_persistent = 1").bind(id),
    env.DB.prepare("DELETE FROM accounts_snapshot WHERE install_id = ?1").bind(id),
    env.DB.prepare(
      "INSERT OR REPLACE INTO forgotten (install_id, forgotten_at) VALUES (?1, ?2)",
    ).bind(id, now),
  ]);

  // Analytics Engine does not support row-level deletion.
  // The forget list is filtered out at admin query time. AE retention is 90 days.
  return json({ ok: true, deleted: true, note: "ae_residual_90d" });
}

// ─── /export (Mode B, GDPR art. 20) ──────────────────────────────

async function handleExport(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_RGPD, ip, "/export", ctx, true);
  if (blocked) return blocked;

  // Parse and validate the body BEFORE touching the daily budget, so malformed
  // or empty requests cannot burn the D1 budget counter. Mirrors handleTrack.
  const parsed = await readJsonCapped<{ install_id?: string }>(request, RGPD_BODY_MAX_BYTES);
  if (parsed instanceof Response) return parsed;
  const payload = parsed;
  if (!payload || !isUuidV4(payload.install_id)) return json({ error: "bad_install_id" }, 400);
  const id = payload.install_id!;

  const budgetErr = await checkBudget(env, "/export", intVar(env.BUDGET_EXPORT, 500), ctx);
  if (budgetErr) return budgetErr;

  const [pings, snapshots] = await Promise.all([
    env.DB.prepare(
      "SELECT date, app_version, os_version, locale, country FROM daily_pings WHERE identifier = ?1 AND is_persistent = 1",
    )
      .bind(id)
      .all(),
    env.DB.prepare("SELECT date, platform, count FROM accounts_snapshot WHERE install_id = ?1")
      .bind(id)
      .all(),
  ]);

  return json({
    install_id: id,
    exported_at: new Date().toISOString(),
    daily_pings: pings.results,
    accounts_snapshot: snapshots.results,
    note: "High-frequency events live in Analytics Engine (90-day retention), not exportable by design.",
  });
}

// ─── /admin/query (read-only D1 SELECT for external dashboard) ───

const ADMIN_QUERY_MAX_ROWS = 1000;
const ADMIN_QUERY_BODY_MAX_BYTES = 64 * 1024;

async function handleAdminQuery(
  request: Request,
  env: Env,
  ctx: ExecutionContext,
): Promise<Response> {
  if (env.ADMIN_ALLOWED !== "true") return json({ error: "disabled" }, 403);
  const authErr = await checkAdminAuth(request, env);
  if (authErr) return authErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_ADMIN, ip, "/admin/query", ctx);
  if (blocked) return blocked;
  const budgetErr = await checkBudget(env, "/admin/query", intVar(env.BUDGET_ADMIN, 2000), ctx);
  if (budgetErr) return budgetErr;

  const parsed = await readJsonCapped<{ sql?: string; params?: unknown[] }>(
    request,
    ADMIN_QUERY_BODY_MAX_BYTES,
  );
  if (parsed instanceof Response) return parsed;
  const payload = parsed;
  if (!payload?.sql || typeof payload.sql !== "string") return json({ error: "bad_sql" }, 400);

  // Read-only: only a plain SELECT is allowed. WITH is rejected outright since
  // a CTE can hide a mutation (`WITH x AS (...) DELETE FROM ...`), and the
  // dashboard never needs one. We also scan the body for any mutation keyword.
  if (!isReadOnlySelect(payload.sql)) {
    return json({ error: "read_only" }, 400);
  }

  // Cap the result set at the DB level. The query must not already carry its
  // own LIMIT (we cannot reliably rewrite an arbitrary one), then we append a
  // hard cap so the database never materializes more than ADMIN_QUERY_MAX_ROWS.
  if (hasLimitClause(payload.sql)) {
    return json({ error: "limit_not_allowed" }, 400);
  }
  // Wrap the caller's SQL in a subquery before appending LIMIT: if the SQL ends
  // with a `--` line comment, appending LIMIT directly after it would land on
  // the commented-out line and never apply. Wrapping keeps LIMIT on the outer,
  // uncommented statement regardless of what the inner query ends with.
  const innerSql = payload.sql.trim().replace(/;\s*$/, "");
  const capped = `SELECT * FROM (${innerSql}) LIMIT ${ADMIN_QUERY_MAX_ROWS}`;

  const stmt = env.DB.prepare(capped);
  const bound =
    Array.isArray(payload.params) && payload.params.length > 0
      ? stmt.bind(...(payload.params as never[]))
      : stmt;
  const result = await bound.all();
  const rows = result.results ?? [];
  const truncated = rows.length >= ADMIN_QUERY_MAX_ROWS;
  return json({ ok: true, results: rows, meta: result.meta, truncated });
}

// Mutation keywords rejected anywhere in an /admin/query body. Matched as whole
// words so a column named e.g. `created_at` does not trip CREATE.
const SQL_MUTATION_KEYWORDS = [
  "DELETE",
  "INSERT",
  "UPDATE",
  "DROP",
  "ALTER",
  "CREATE",
  "REPLACE",
  "ATTACH",
  "PRAGMA",
];

const SQL_MUTATION_RE = new RegExp(`\\b(?:${SQL_MUTATION_KEYWORDS.join("|")})\\b`, "i");

// True only for a plain SELECT with no mutation keyword anywhere in the body.
// WITH (CTE) is rejected: it can prefix a DELETE/UPDATE and the dashboard does
// not use it.
function isReadOnlySelect(sql: string): boolean {
  const trimmed = sql.trim();
  if (!/^SELECT\b/i.test(trimmed)) return false;
  if (SQL_MUTATION_RE.test(trimmed)) return false;
  return true;
}

// True if the SQL already contains a LIMIT clause (as a word), so we refuse it
// rather than appending a second LIMIT that SQLite would reject.
function hasLimitClause(sql: string): boolean {
  return /\bLIMIT\b/i.test(sql);
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

// Bearer token check for /admin/* endpoints. Runs before rate limiting and
// budget counters so unauthenticated probes cannot consume them.
async function checkAdminAuth(request: Request, env: Env): Promise<Response | null> {
  const auth = request.headers.get("Authorization") ?? "";
  const token = auth.startsWith("Bearer ") ? auth.slice(7) : "";
  if (!token || !(await timingSafeEqual(token, env.ADMIN_TOKEN))) {
    return json({ error: "unauthorized" }, 401);
  }
  return null;
}

// Constant-time string comparison. Hashing both sides first equalizes the
// lengths, so the XOR loop leaks nothing about where the strings differ.
async function timingSafeEqual(a: string, b: string): Promise<boolean> {
  const enc = new TextEncoder();
  const [da, db] = await Promise.all([
    crypto.subtle.digest("SHA-256", enc.encode(a)),
    crypto.subtle.digest("SHA-256", enc.encode(b)),
  ]);
  const ua = new Uint8Array(da);
  const ub = new Uint8Array(db);
  let diff = 0;
  for (let i = 0; i < ua.length; i++) diff |= ua[i]! ^ ub[i]!;
  return diff === 0;
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

// ─── Anti-abuse: UA filter + global daily budget ─────────────────

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

async function checkBudget(
  env: Env,
  endpoint: string,
  cap: number,
  ctx: ExecutionContext,
): Promise<Response | null> {
  const today = new Date().toISOString().slice(0, 10);
  try {
    const r = await env.DB.prepare(
      `INSERT INTO global_budget (date, endpoint, count) VALUES (?1, ?2, 1)
         ON CONFLICT(date, endpoint) DO UPDATE SET count = count + 1
         RETURNING count`,
    )
      .bind(today, endpoint)
      .first<{ count: number }>();
    const count = r?.count ?? 0;
    if (count > cap) {
      // Already over cap: just reject. The single alert was sent when count === cap,
      // so we never re-notify here (notifyBudget has no rate limit, a flood would spam Resend).
      return json({ error: "daily_budget_reached", endpoint }, 503);
    }
    // One-shot notification at threshold crossing, not on every request beyond.
    if (count === cap) ctx.waitUntil(notifyBudget(env, endpoint, count, cap));
    return null;
  } catch (e) {
    // On D1 error, let the request through. Availability over caps.
    console.error("budget check failed", e);
    return null;
  }
}

async function notifyBudget(env: Env, endpoint: string, count: number, cap: number): Promise<void> {
  if (!env.RESEND_API_KEY || !env.ALERT_EMAIL) return;
  const subject = `[Accshift telemetry] daily budget reached on ${endpoint}`;
  const text = [
    `Endpoint: ${endpoint}`,
    `Counter: ${count} / cap ${cap}`,
    `When: ${new Date().toISOString()}`,
    ``,
    `The Worker now returns 503 on this endpoint until the end of the UTC day.`,
    `No Cloudflare charge happened thanks to this cap.`,
    `Check the Worker logs and consider adjusting the cap if the traffic is legitimate.`,
  ].join("\n");
  try {
    await fetch("https://api.resend.com/emails", {
      method: "POST",
      headers: {
        Authorization: `Bearer ${env.RESEND_API_KEY}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ from: env.ALERT_FROM, to: [env.ALERT_EMAIL], subject, text }),
    });
  } catch (e) {
    console.error("budget notify failed", e);
  }
}

// ─── Rate limiting + email alerts ────────────────────────────────

function clientIp(request: Request): string {
  return request.headers.get("CF-Connecting-IP") ?? "";
}

// Mask an IP keeping only the /24 (v4) or /48 (v6) prefix so an alert email
// gives a coarse geographic hint without being a full PII.
function maskIp(ip: string): string {
  if (!ip) return "unknown";
  if (ip.includes(":")) {
    const parts = ip.split(":").slice(0, 3);
    return parts.join(":") + "::/48";
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
    // No trustworthy IP (e.g. local dev). Public write endpoints fail closed;
    // admin endpoints stay open since the bearer token already gates them.
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
