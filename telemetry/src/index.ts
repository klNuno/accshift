// Accshift telemetry Worker
// =========================
// Endpoints:
//   POST /track        - Mode A or B events (batch)
//   POST /logs         - log zip upload (explicit user action)
//   POST /forget       - GDPR art. 17, Mode B only
//   POST /export       - GDPR art. 20, Mode B only
//   POST /admin/query  - read-only D1 SELECT for an external dashboard
//
// Principles:
// - Mode A: no persistent identifier stored on the client. The server derives
//   a daily_visitor_hash from IP + UA + date + secret. The IP is never stored,
//   and the hash is not recomputable across days (date is part of the input).
// - IP is never stored. Country is derived from request.cf.country.
// - CORS open: Tauri does not send a reliable Origin for native apps.

interface RateLimit {
  limit(options: { key: string }): Promise<{ success: boolean }>;
}

export interface Env {
  AE: AnalyticsEngineDataset;
  DB: D1Database;
  LOGS: R2Bucket;
  RL_TRACK: RateLimit;
  RL_LOGS: RateLimit;
  RL_RGPD: RateLimit;
  RL_ADMIN: RateLimit;
  RL_NOTIFY: RateLimit;
  HASH_SECRET: string;
  ADMIN_TOKEN: string;
  RESEND_API_KEY: string;
  ALERT_EMAIL: string;
  ALERT_FROM: string;
  ENVIRONMENT: string;
  LOG_MAX_BYTES: string;
  BATCH_MAX_EVENTS: string;
  ADMIN_ALLOWED: string;
  UA_PREFIX: string;
  BUDGET_TRACK: string;
  BUDGET_LOGS: string;
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
  events: TelemetryEvent[];
}

// ─── Entry point ─────────────────────────────────────────────────
export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    if (request.method === "OPTIONS") return cors(new Response(null, { status: 204 }));

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
        case "/logs":
          response =
            request.method === "POST" ? await handleLogs(request, env, ctx) : methodNotAllowed();
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
      return cors(response);
    } catch (err) {
      console.error("unhandled", err);
      return cors(json({ error: "internal_error" }, 500));
    }
  },
};

// ─── /track ──────────────────────────────────────────────────────
async function handleTrack(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_TRACK, ip, "/track", ctx);
  if (blocked) return blocked;
  const budgetErr = await checkBudget(env, "/track", intVar(env.BUDGET_TRACK, 4000), ctx);
  if (budgetErr) return budgetErr;

  const payload = await safeJson<TrackPayload>(request);
  if (!payload || !Array.isArray(payload.events) || payload.events.length === 0) {
    return json({ error: "bad_payload" }, 400);
  }

  const maxEvents = parseInt(env.BATCH_MAX_EVENTS, 10) || 200;
  if (payload.events.length > maxEvents) {
    return json({ error: "batch_too_large", max: maxEvents }, 413);
  }

  if (payload.mode !== "A" && payload.mode !== "B") {
    return json({ error: "bad_mode" }, 400);
  }
  if (payload.mode === "B" && !isUuidV4(payload.install_id)) {
    return json({ error: "bad_install_id" }, 400);
  }

  const country = (request.cf?.country as string | undefined) ?? "XX";
  const todayIso = new Date().toISOString().slice(0, 10);

  let identifier: string;
  let isPersistent: 0 | 1;
  if (payload.mode === "B") {
    identifier = payload.install_id!;
    isPersistent = 1;
    // GDPR: drop the batch if the install_id is in the forget list.
    const forgotten = await env.DB.prepare("SELECT 1 FROM forgotten WHERE install_id = ?1")
      .bind(identifier)
      .first();
    if (forgotten) return json({ ok: true, forgotten: true });
  } else {
    const ua = request.headers.get("User-Agent") ?? "";
    identifier = await dailyVisitorHash(ip, ua, todayIso, env.HASH_SECRET);
    isPersistent = 0;
  }

  // Analytics Engine: one datapoint per event.
  for (const ev of payload.events) {
    if (!ev.name || typeof ev.name !== "string") continue;
    env.AE.writeDataPoint({
      indexes: [identifier.slice(0, 96)],
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
        identifier,
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
        ).bind(identifier, todayIso, s.platform!, Math.max(0, Math.floor(s.count ?? 0))),
      );
      await env.DB.batch(stmts);
    }
  }

  return json({ ok: true, accepted: payload.events.length });
}

// ─── /logs ───────────────────────────────────────────────────────
async function handleLogs(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_LOGS, ip, "/logs", ctx);
  if (blocked) return blocked;
  const budgetErr = await checkBudget(env, "/logs", intVar(env.BUDGET_LOGS, 2000), ctx);
  if (budgetErr) return budgetErr;

  const maxBytes = parseInt(env.LOG_MAX_BYTES, 10) || 10 * 1024 * 1024;
  const lenHeader = request.headers.get("Content-Length");
  if (lenHeader && parseInt(lenHeader, 10) > maxBytes) {
    return json({ error: "payload_too_large", max: maxBytes }, 413);
  }

  const contentType = request.headers.get("Content-Type") ?? "application/zip";
  if (!contentType.includes("zip") && !contentType.includes("octet-stream")) {
    return json({ error: "bad_content_type" }, 415);
  }

  const body = await request.arrayBuffer();
  if (body.byteLength === 0) return json({ error: "empty_body" }, 400);
  if (body.byteLength > maxBytes) return json({ error: "payload_too_large", max: maxBytes }, 413);

  const ticketId = makeTicketId();
  const yyyymm = new Date().toISOString().slice(0, 7);
  const key = `logs/${yyyymm}/${ticketId}.zip`;

  const appVersion = request.headers.get("X-App-Version") ?? "";
  const osVersion = request.headers.get("X-OS-Version") ?? "";
  const country = (request.cf?.country as string | undefined) ?? "XX";
  const note = decodeNoteHeader(request.headers.get("X-Note-B64"));

  await env.LOGS.put(key, body, {
    httpMetadata: { contentType: "application/zip" },
    customMetadata: { app_version: appVersion, os_version: osVersion, country },
  });

  await env.DB.prepare(
    `INSERT INTO log_uploads (ticket_id, created_at, size_bytes, app_version, os_version, country, note)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)`,
  )
    .bind(
      ticketId,
      Math.floor(Date.now() / 1000),
      body.byteLength,
      appVersion,
      osVersion,
      country,
      note,
    )
    .run();

  return json({ ok: true, ticket_id: ticketId });
}

const NOTE_MAX_CHARS = 1000;

// Decodes the base64 note header into a plain string, capped at NOTE_MAX_CHARS
// characters. Invalid base64 or oversized payloads degrade to null instead of
// failing the upload, since the note is auxiliary metadata.
function decodeNoteHeader(raw: string | null): string | null {
  if (!raw) return null;
  try {
    const bytes = Uint8Array.from(atob(raw), (c) => c.charCodeAt(0));
    const decoded = new TextDecoder("utf-8", { fatal: false, ignoreBOM: false })
      .decode(bytes)
      .trim();
    if (!decoded) return null;
    return decoded.length > NOTE_MAX_CHARS ? decoded.slice(0, NOTE_MAX_CHARS) : decoded;
  } catch {
    return null;
  }
}

// ─── /forget (Mode B) ────────────────────────────────────────────

async function handleForget(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_RGPD, ip, "/forget", ctx);
  if (blocked) return blocked;
  const budgetErr = await checkBudget(env, "/forget", intVar(env.BUDGET_FORGET, 500), ctx);
  if (budgetErr) return budgetErr;

  const payload = await safeJson<{ install_id?: string }>(request);
  if (!payload || !isUuidV4(payload.install_id)) return json({ error: "bad_install_id" }, 400);

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
  const blocked = await enforceRateLimit(env, env.RL_RGPD, ip, "/export", ctx);
  if (blocked) return blocked;
  const budgetErr = await checkBudget(env, "/export", intVar(env.BUDGET_EXPORT, 500), ctx);
  if (budgetErr) return budgetErr;

  const payload = await safeJson<{ install_id?: string }>(request);
  if (!payload || !isUuidV4(payload.install_id)) return json({ error: "bad_install_id" }, 400);
  const id = payload.install_id!;

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

async function handleAdminQuery(
  request: Request,
  env: Env,
  ctx: ExecutionContext,
): Promise<Response> {
  if (env.ADMIN_ALLOWED !== "true") return json({ error: "disabled" }, 403);
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_ADMIN, ip, "/admin/query", ctx);
  if (blocked) return blocked;
  const budgetErr = await checkBudget(env, "/admin/query", intVar(env.BUDGET_ADMIN, 2000), ctx);
  if (budgetErr) return budgetErr;

  const auth = request.headers.get("Authorization") ?? "";
  if (!auth.startsWith("Bearer ") || auth.slice(7) !== env.ADMIN_TOKEN) {
    return json({ error: "unauthorized" }, 401);
  }

  const payload = await safeJson<{ sql?: string; params?: unknown[] }>(request);
  if (!payload?.sql || typeof payload.sql !== "string") return json({ error: "bad_sql" }, 400);

  // Read-only: reject anything that is not SELECT or WITH.
  const normalized = payload.sql.trim().toUpperCase();
  if (!normalized.startsWith("SELECT") && !normalized.startsWith("WITH")) {
    return json({ error: "read_only" }, 400);
  }

  const stmt = env.DB.prepare(payload.sql);
  const bound =
    Array.isArray(payload.params) && payload.params.length > 0
      ? stmt.bind(...(payload.params as never[]))
      : stmt;
  const result = await bound.all();
  return json({ ok: true, results: result.results, meta: result.meta });
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

function bytesToHex(bytes: Uint8Array): string {
  const HEX = "0123456789abcdef";
  let s = "";
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i]!;
    s += HEX[b >> 4]! + HEX[b & 0x0f]!;
  }
  return s;
}

function makeTicketId(): string {
  // 6 chars Crockford base32 (no I, L, O, U).
  const alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
  const rnd = crypto.getRandomValues(new Uint8Array(6));
  let out = "";
  for (let i = 0; i < 6; i++) out += alphabet[rnd[i]! % 32];
  return out;
}

function isUuidV4(s: unknown): s is string {
  return (
    typeof s === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(s)
  );
}

async function safeJson<T>(request: Request): Promise<T | null> {
  try {
    return (await request.json()) as T;
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

function cors(res: Response): Response {
  const h = new Headers(res.headers);
  h.set("Access-Control-Allow-Origin", "*");
  h.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
  h.set(
    "Access-Control-Allow-Headers",
    "Content-Type, Authorization, X-App-Version, X-OS-Version, X-Note-B64",
  );
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
      ctx.waitUntil(notifyBudget(env, endpoint, count, cap));
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
): Promise<Response | null> {
  if (!ip) return null; // skip RL when no trustworthy IP, e.g. local dev.
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
