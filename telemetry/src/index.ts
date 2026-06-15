// Accshift telemetry Worker
// =========================
// Endpoints:
//   POST /track             - Mode A or B events (batch)
//   POST /logs              - log zip upload (explicit user action)
//   POST /forget            - GDPR art. 17, Mode B only
//   POST /export            - GDPR art. 20, Mode B only
//   POST /admin/query       - read-only D1 SELECT for an external dashboard
//   GET  /admin/dashboard   - inline HTML dashboard for log triage
//   GET  /admin/logs/<id>   - stream the R2 zip for a ticket_id
//   (cron)                  - daily purge of log uploads older than 90 days
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
        case "/admin/dashboard":
          response = request.method === "GET" ? handleAdminDashboard(env) : methodNotAllowed();
          break;
        default: {
          const logsMatch = url.pathname.match(/^\/admin\/logs\/([0-9A-Z]{4,32})$/);
          if (logsMatch) {
            response =
              request.method === "GET"
                ? await handleAdminLog(request, env, ctx, logsMatch[1]!)
                : methodNotAllowed();
            break;
          }
          response = json({ error: "not_found" }, 404);
        }
      }
      return cors(response);
    } catch (err) {
      console.error("unhandled", err);
      return cors(json({ error: "internal_error" }, 500));
    }
  },

  async scheduled(
    _controller: ScheduledController,
    env: Env,
    ctx: ExecutionContext,
  ): Promise<void> {
    ctx.waitUntil(purgeExpiredLogs(env));
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

  // Payload is well-formed and carries at least one usable event: now charge
  // the daily budget.
  const budgetErr = await checkBudget(env, "/track", intVar(env.BUDGET_TRACK, 4000), ctx);
  if (budgetErr) return budgetErr;

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
  const blocked = await enforceRateLimit(env, env.RL_LOGS, ip, "/logs", ctx, true);
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

  const createdAt = Math.floor(Date.now() / 1000);
  const appVersion = request.headers.get("X-App-Version") ?? "";
  const osVersion = request.headers.get("X-OS-Version") ?? "";
  const country = (request.cf?.country as string | undefined) ?? "XX";
  const note = decodeNoteHeader(request.headers.get("X-Note-B64"));

  // D1 first: the PK on ticket_id turns a collision into an INSERT failure
  // instead of silently overwriting another user's R2 object. Retry with a
  // fresh id on conflict.
  let ticketId = "";
  let inserted = false;
  for (let attempt = 0; attempt < 3 && !inserted; attempt++) {
    ticketId = makeTicketId();
    try {
      await env.DB.prepare(
        `INSERT INTO log_uploads (ticket_id, created_at, size_bytes, app_version, os_version, country, note)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)`,
      )
        .bind(ticketId, createdAt, body.byteLength, appVersion, osVersion, country, note)
        .run();
      inserted = true;
    } catch (e) {
      if (!isUniqueConstraintError(e)) throw e;
    }
  }
  if (!inserted) return json({ error: "internal_error" }, 500);

  const yyyymm = new Date(createdAt * 1000).toISOString().slice(0, 7);
  const key = `logs/${yyyymm}/${ticketId}.zip`;
  try {
    await env.LOGS.put(key, body, {
      httpMetadata: { contentType: "application/zip" },
      customMetadata: { app_version: appVersion, os_version: osVersion, country },
    });
  } catch (e) {
    console.error("r2 put failed", e);
    // Best effort: drop the index row so it never points at a missing object.
    try {
      await env.DB.prepare("DELETE FROM log_uploads WHERE ticket_id = ?1").bind(ticketId).run();
    } catch {
      // leave the orphan row; /admin/logs answers 404 for it
    }
    return json({ error: "internal_error" }, 500);
  }

  return json({ ok: true, ticket_id: ticketId });
}

function isUniqueConstraintError(e: unknown): boolean {
  const cause = e instanceof Error && e.cause instanceof Error ? e.cause.message : "";
  const msg = e instanceof Error ? `${e.message} ${cause}` : String(e);
  return msg.includes("UNIQUE constraint failed");
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

// ─── Log retention (daily cron) ──────────────────────────────────

const LOG_RETENTION_DAYS = 90;
const LOG_PURGE_BATCH = 500;

// Deletes log uploads older than LOG_RETENTION_DAYS, capped at LOG_PURGE_BATCH
// per run. R2 objects go first so a failed run leaves the D1 index rows in
// place and the next cron retries. The note lives on the same row, so deleting
// the row removes it too.
async function purgeExpiredLogs(env: Env): Promise<void> {
  const cutoff = Math.floor(Date.now() / 1000) - LOG_RETENTION_DAYS * 86400;
  const expired = await env.DB.prepare(
    `SELECT ticket_id, created_at FROM log_uploads
       WHERE created_at < ?1 ORDER BY created_at LIMIT ${LOG_PURGE_BATCH}`,
  )
    .bind(cutoff)
    .all<{ ticket_id: string; created_at: number }>();
  const rows = expired.results ?? [];
  if (rows.length === 0) return;

  const keys = rows.map((r) => {
    const yyyymm = new Date(r.created_at * 1000).toISOString().slice(0, 7);
    return `logs/${yyyymm}/${r.ticket_id}.zip`;
  });
  await env.LOGS.delete(keys); // bulk delete, missing keys are no-ops

  // D1 caps bound parameters per statement; delete in chunks via batch.
  const stmts: D1PreparedStatement[] = [];
  for (let i = 0; i < rows.length; i += 50) {
    const chunk = rows.slice(i, i + 50);
    stmts.push(
      env.DB.prepare(
        `DELETE FROM log_uploads WHERE ticket_id IN (${chunk.map(() => "?").join(", ")})`,
      ).bind(...chunk.map((r) => r.ticket_id)),
    );
  }
  await env.DB.batch(stmts);
  console.log(`log retention: purged ${rows.length} uploads older than ${LOG_RETENTION_DAYS}d`);
}

// ─── /forget (Mode B) ────────────────────────────────────────────

const RGPD_BODY_MAX_BYTES = 4 * 1024;

async function handleForget(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const uaErr = checkUa(request, env);
  if (uaErr) return uaErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_RGPD, ip, "/forget", ctx);
  if (blocked) return blocked;
  const budgetErr = await checkBudget(env, "/forget", intVar(env.BUDGET_FORGET, 500), ctx);
  if (budgetErr) return budgetErr;

  const parsed = await readJsonCapped<{ install_id?: string }>(request, RGPD_BODY_MAX_BYTES);
  if (parsed instanceof Response) return parsed;
  const payload = parsed;
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

  const parsed = await readJsonCapped<{ install_id?: string }>(request, RGPD_BODY_MAX_BYTES);
  if (parsed instanceof Response) return parsed;
  const payload = parsed;
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

const ADMIN_QUERY_MAX_ROWS = 1000;

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

  const payload = await safeJson<{ sql?: string; params?: unknown[] }>(request);
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
  const capped = `${payload.sql.trim().replace(/;\s*$/, "")} LIMIT ${ADMIN_QUERY_MAX_ROWS}`;

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

// ─── /admin/logs/<ticket_id> (stream R2 zip) ─────────────────────

async function handleAdminLog(
  request: Request,
  env: Env,
  ctx: ExecutionContext,
  ticketId: string,
): Promise<Response> {
  if (env.ADMIN_ALLOWED !== "true") return json({ error: "disabled" }, 403);
  const authErr = await checkAdminAuth(request, env);
  if (authErr) return authErr;
  const ip = clientIp(request);
  const blocked = await enforceRateLimit(env, env.RL_ADMIN, ip, "/admin/logs", ctx);
  if (blocked) return blocked;
  const budgetErr = await checkBudget(env, "/admin/logs", intVar(env.BUDGET_ADMIN, 2000), ctx);
  if (budgetErr) return budgetErr;

  const row = await env.DB.prepare(`SELECT created_at FROM log_uploads WHERE ticket_id = ?1`)
    .bind(ticketId)
    .first<{ created_at: number }>();
  if (!row) return json({ error: "not_found" }, 404);

  const yyyymm = new Date(row.created_at * 1000).toISOString().slice(0, 7);
  const key = `logs/${yyyymm}/${ticketId}.zip`;
  const obj = await env.LOGS.get(key);
  if (!obj) return json({ error: "r2_missing" }, 404);

  const headers = new Headers();
  headers.set("Content-Type", "application/zip");
  headers.set("Content-Disposition", `attachment; filename="${ticketId}.zip"`);
  if (obj.size) headers.set("Content-Length", String(obj.size));
  headers.set("Cache-Control", "private, no-store");
  return new Response(obj.body, { headers });
}

// ─── /admin/dashboard (inline HTML) ──────────────────────────────

function handleAdminDashboard(_env: Env): Response {
  return new Response(ADMIN_DASHBOARD_HTML, {
    headers: {
      "Content-Type": "text/html; charset=utf-8",
      "Cache-Control": "private, no-store",
      "X-Frame-Options": "DENY",
      "Referrer-Policy": "no-referrer",
    },
  });
}

const ADMIN_DASHBOARD_HTML = String.raw`<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex,nofollow">
<title>Accshift — Log triage</title>
<style>
  :root {
    color-scheme: dark;
    --bg: #0b0d10;
    --panel: #14181d;
    --panel-2: #1a1f26;
    --line: #262d36;
    --text: #e6e8eb;
    --muted: #8a93a0;
    --accent: #7ab8ff;
    --warn: #ffb454;
    --err: #ff6b6b;
    --ok: #5fd07a;
  }
  * { box-sizing: border-box; }
  body { margin: 0; background: var(--bg); color: var(--text); font: 13px/1.45 ui-sans-serif, system-ui, sans-serif; }
  header { display: flex; align-items: center; gap: 16px; padding: 12px 16px; border-bottom: 1px solid var(--line); background: var(--panel); position: sticky; top: 0; z-index: 10; }
  header h1 { font-size: 15px; margin: 0; font-weight: 600; letter-spacing: 0.2px; }
  header .spacer { flex: 1; }
  header button { background: var(--panel-2); color: var(--text); border: 1px solid var(--line); border-radius: 6px; padding: 6px 12px; cursor: pointer; font: inherit; }
  header button:hover { border-color: var(--accent); }
  .filters { display: flex; gap: 8px; flex-wrap: wrap; padding: 12px 16px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .filters input, .filters select { background: var(--panel-2); color: var(--text); border: 1px solid var(--line); border-radius: 6px; padding: 6px 10px; font: inherit; min-width: 0; }
  .filters input { width: 160px; }
  .filters .grow { flex: 1; min-width: 200px; }
  table { width: 100%; border-collapse: collapse; font-size: 12.5px; }
  th, td { padding: 6px 10px; text-align: left; border-bottom: 1px solid var(--line); white-space: nowrap; }
  th { background: var(--panel); color: var(--muted); font-weight: 500; position: sticky; top: 88px; cursor: pointer; user-select: none; }
  th:hover { color: var(--text); }
  tbody tr { cursor: pointer; }
  tbody tr:hover { background: var(--panel-2); }
  td.note { white-space: normal; max-width: 480px; color: var(--muted); }
  .pill { display: inline-block; padding: 1px 6px; border-radius: 4px; font-size: 11px; background: var(--panel-2); color: var(--muted); border: 1px solid var(--line); }
  .empty { padding: 32px 16px; text-align: center; color: var(--muted); }
  dialog { background: var(--panel); color: var(--text); border: 1px solid var(--line); border-radius: 8px; padding: 0; max-width: 90vw; max-height: 88vh; width: 1100px; }
  dialog::backdrop { background: rgba(0,0,0,0.6); }
  dialog header { background: var(--panel-2); border-bottom: 1px solid var(--line); position: static; }
  dialog .body { padding: 12px 16px; overflow: auto; max-height: 72vh; }
  dialog footer { padding: 8px 12px; border-top: 1px solid var(--line); display: flex; gap: 8px; }
  pre.log { background: #0a0c0f; padding: 8px; border-radius: 6px; font-size: 12px; white-space: pre-wrap; word-break: break-word; max-height: 60vh; overflow: auto; border: 1px solid var(--line); }
  .row-meta { display: flex; gap: 12px; padding: 6px 16px; background: var(--panel-2); border-bottom: 1px solid var(--line); font-size: 12px; color: var(--muted); }
  .row-meta b { color: var(--text); font-weight: 500; }
  .lvl-error { color: var(--err); }
  .lvl-warn { color: var(--warn); }
  .lvl-info { color: var(--accent); }
  .lvl-debug { color: var(--muted); }
  .tab-bar { display: flex; gap: 4px; padding: 8px 12px 0; }
  .tab-bar button { background: transparent; color: var(--muted); border: 1px solid transparent; border-bottom: none; border-radius: 6px 6px 0 0; padding: 6px 12px; cursor: pointer; font: inherit; }
  .tab-bar button.active { background: #0a0c0f; color: var(--text); border-color: var(--line); }
  .status { padding: 6px 12px; font-size: 11px; color: var(--muted); }
</style>
</head>
<body>
<header>
  <h1>Accshift · log triage</h1>
  <span class="status" id="status">loading…</span>
  <span class="spacer"></span>
  <button id="reload">↻ Reload</button>
  <button id="logout">⎋ Forget token</button>
</header>
<div class="filters">
  <input id="f-version" placeholder="app version (substring)">
  <input id="f-os" placeholder="OS version (substring)">
  <input id="f-country" placeholder="country (2-letter)">
  <input id="f-from" type="date">
  <input id="f-to" type="date">
  <input id="f-note" class="grow" placeholder="note contains…">
</div>
<table>
  <thead><tr>
    <th data-sort="created_at">When</th>
    <th data-sort="ticket_id">Ticket</th>
    <th data-sort="app_version">Version</th>
    <th data-sort="os_version">OS</th>
    <th data-sort="country">Country</th>
    <th data-sort="size_bytes">Size</th>
    <th>Note</th>
  </tr></thead>
  <tbody id="rows"></tbody>
</table>
<div class="empty" id="empty" style="display:none">No uploads matching the filters.</div>

<dialog id="modal">
  <header>
    <h1 id="m-title">Ticket</h1>
    <span class="spacer"></span>
    <button id="m-download">⬇ Download zip</button>
    <button id="m-copy">⧉ Copy id</button>
    <button id="m-close">✕</button>
  </header>
  <div class="row-meta" id="m-meta"></div>
  <div class="tab-bar" id="m-tabs"></div>
  <div class="body">
    <div class="filters" style="border:none;padding:0 0 8px;background:transparent">
      <select id="m-level">
        <option value="">all levels</option>
        <option value="error">error</option>
        <option value="warn">warn</option>
        <option value="info">info</option>
        <option value="debug">debug</option>
      </select>
      <input id="m-source" placeholder="source contains…" class="grow">
      <input id="m-msg" placeholder="message contains…" class="grow">
    </div>
    <pre class="log" id="m-out">Loading…</pre>
  </div>
</dialog>

<script>
"use strict";

const TOKEN_KEY = "accshift_admin_token";

function getToken() {
  let t = sessionStorage.getItem(TOKEN_KEY);
  if (!t) {
    t = prompt("Admin token (Bearer)");
    if (!t) throw new Error("no_token");
    sessionStorage.setItem(TOKEN_KEY, t);
  }
  return t;
}
function forgetToken() { sessionStorage.removeItem(TOKEN_KEY); location.reload(); }

async function authFetch(url, init) {
  const token = getToken();
  const headers = new Headers(init && init.headers);
  headers.set("Authorization", "Bearer " + token);
  const res = await fetch(url, Object.assign({}, init, { headers }));
  if (res.status === 401) { sessionStorage.removeItem(TOKEN_KEY); throw new Error("unauthorized"); }
  return res;
}

let state = {
  rows: [],
  sortKey: "created_at",
  sortDir: -1,
  filters: { version: "", os: "", country: "", from: "", to: "", note: "" },
};

async function loadRows() {
  document.getElementById("status").textContent = "loading…";
  try {
    const res = await authFetch("/admin/query", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sql: "SELECT ticket_id, created_at, size_bytes, app_version, os_version, country, note FROM log_uploads ORDER BY created_at DESC" }),
    });
    if (!res.ok) throw new Error("HTTP " + res.status);
    const data = await res.json();
    state.rows = data.results || [];
    document.getElementById("status").textContent = state.rows.length + " uploads";
    render();
  } catch (e) {
    document.getElementById("status").textContent = "error: " + e.message;
    if (e.message === "unauthorized" || e.message === "no_token") setTimeout(() => location.reload(), 50);
  }
}

function fmtBytes(n) {
  if (n < 1024) return n + " B";
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + " KB";
  return (n / 1024 / 1024).toFixed(2) + " MB";
}
function fmtDate(secs) {
  const d = new Date(secs * 1000);
  const pad = (x) => String(x).padStart(2, "0");
  return d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()) + " " + pad(d.getHours()) + ":" + pad(d.getMinutes());
}
function esc(s) { return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])); }

function applyFilters(rows) {
  const f = state.filters;
  return rows.filter((r) => {
    if (f.version && !(r.app_version || "").toLowerCase().includes(f.version.toLowerCase())) return false;
    if (f.os && !(r.os_version || "").toLowerCase().includes(f.os.toLowerCase())) return false;
    if (f.country && (r.country || "").toLowerCase() !== f.country.toLowerCase()) return false;
    if (f.note && !(r.note || "").toLowerCase().includes(f.note.toLowerCase())) return false;
    if (f.from) {
      const ts = Math.floor(new Date(f.from + "T00:00:00Z").getTime() / 1000);
      if (r.created_at < ts) return false;
    }
    if (f.to) {
      const ts = Math.floor(new Date(f.to + "T23:59:59Z").getTime() / 1000);
      if (r.created_at > ts) return false;
    }
    return true;
  });
}

function render() {
  const tbody = document.getElementById("rows");
  const filtered = applyFilters(state.rows);
  const sorted = filtered.slice().sort((a, b) => {
    const k = state.sortKey;
    const av = a[k] ?? "";
    const bv = b[k] ?? "";
    if (av < bv) return -1 * state.sortDir;
    if (av > bv) return 1 * state.sortDir;
    return 0;
  });
  document.getElementById("empty").style.display = sorted.length ? "none" : "block";
  tbody.innerHTML = sorted.map((r) => (
    '<tr data-ticket="' + esc(r.ticket_id) + '" data-when="' + r.created_at + '">' +
    '<td>' + fmtDate(r.created_at) + '</td>' +
    '<td><span class="pill">' + esc(r.ticket_id) + '</span></td>' +
    '<td>' + esc(r.app_version) + '</td>' +
    '<td>' + esc(r.os_version) + '</td>' +
    '<td>' + esc(r.country || "—") + '</td>' +
    '<td>' + fmtBytes(r.size_bytes) + '</td>' +
    '<td class="note">' + esc(r.note || "") + '</td>' +
    '</tr>'
  )).join("");
}

document.getElementById("rows").addEventListener("click", (e) => {
  const tr = e.target.closest("tr");
  if (!tr) return;
  openTicket(tr.dataset.ticket);
});
document.querySelectorAll("th[data-sort]").forEach((th) => {
  th.addEventListener("click", () => {
    const k = th.dataset.sort;
    if (state.sortKey === k) state.sortDir *= -1; else { state.sortKey = k; state.sortDir = -1; }
    render();
  });
});
["version", "os", "country", "from", "to", "note"].forEach((k) => {
  document.getElementById("f-" + k).addEventListener("input", (e) => {
    state.filters[k] = e.target.value;
    render();
  });
});
document.getElementById("reload").addEventListener("click", loadRows);
document.getElementById("logout").addEventListener("click", forgetToken);

// ─── Ticket modal ────────────────────────────────────────────────

let currentTicket = null;
let currentFiles = new Map();
let currentTab = null;

async function openTicket(ticketId) {
  currentTicket = ticketId;
  const modal = document.getElementById("modal");
  document.getElementById("m-title").textContent = "Ticket " + ticketId;
  document.getElementById("m-out").textContent = "Downloading…";
  document.getElementById("m-meta").innerHTML = "";
  document.getElementById("m-tabs").innerHTML = "";
  modal.showModal();
  try {
    const res = await authFetch("/admin/logs/" + encodeURIComponent(ticketId));
    if (!res.ok) throw new Error("HTTP " + res.status);
    const buf = await res.arrayBuffer();
    currentFiles = await parseZip(buf);
    if (currentFiles.size === 0) { document.getElementById("m-out").textContent = "(zip empty)"; return; }
    const names = Array.from(currentFiles.keys());
    currentTab = names[0];
    renderTabs(names);
    renderModalMeta();
    renderModalLog();
  } catch (e) {
    document.getElementById("m-out").textContent = "Error: " + e.message;
  }
}

function renderTabs(names) {
  const bar = document.getElementById("m-tabs");
  bar.innerHTML = names.map((n) => '<button data-file="' + esc(n) + '"' + (n === currentTab ? ' class="active"' : "") + ">" + esc(n) + "</button>").join("");
  bar.querySelectorAll("button").forEach((b) => {
    b.addEventListener("click", () => { currentTab = b.dataset.file; renderTabs(names); renderModalLog(); });
  });
}

function renderModalMeta() {
  const row = state.rows.find((r) => r.ticket_id === currentTicket);
  if (!row) return;
  document.getElementById("m-meta").innerHTML =
    '<span><b>When:</b> ' + fmtDate(row.created_at) + '</span>' +
    '<span><b>Version:</b> ' + esc(row.app_version) + '</span>' +
    '<span><b>OS:</b> ' + esc(row.os_version) + '</span>' +
    '<span><b>Country:</b> ' + esc(row.country || "—") + '</span>' +
    '<span><b>Size:</b> ' + fmtBytes(row.size_bytes) + '</span>' +
    (row.note ? '<span><b>Note:</b> ' + esc(row.note) + '</span>' : "");
}

function renderModalLog() {
  const bytes = currentFiles.get(currentTab);
  if (!bytes) { document.getElementById("m-out").textContent = "(empty)"; return; }
  const text = new TextDecoder("utf-8", { fatal: false }).decode(bytes);
  const level = document.getElementById("m-level").value;
  const sourceQ = document.getElementById("m-source").value.toLowerCase();
  const msgQ = document.getElementById("m-msg").value.toLowerCase();
  const lines = text.split("\n");
  const out = [];
  for (const line of lines) {
    if (!line.trim()) continue;
    let rec;
    try { rec = JSON.parse(line); } catch { out.push(esc(line)); continue; }
    if (level && rec.level !== level) continue;
    if (sourceQ && !(rec.source || "").toLowerCase().includes(sourceQ)) continue;
    if (msgQ && !(rec.message || "").toLowerCase().includes(msgQ)) continue;
    const cls = "lvl-" + (rec.level || "info");
    const ts = rec.tsMs ? new Date(rec.tsMs).toISOString().replace("T", " ").slice(0, 19) : "—";
    out.push(
      '<span class="' + cls + '">[' + esc(rec.level || "?") + ']</span> ' +
      ts + ' ' + esc(rec.source || "?") + ' — ' + esc(rec.message || "") +
      (rec.details ? "\n    " + esc(String(rec.details)) : "")
    );
  }
  document.getElementById("m-out").innerHTML = out.join("\n");
}

["m-level", "m-source", "m-msg"].forEach((id) => document.getElementById(id).addEventListener("input", renderModalLog));
document.getElementById("m-close").addEventListener("click", () => document.getElementById("modal").close());
document.getElementById("m-copy").addEventListener("click", () => navigator.clipboard.writeText(currentTicket));
document.getElementById("m-download").addEventListener("click", async () => {
  const res = await authFetch("/admin/logs/" + encodeURIComponent(currentTicket));
  const blob = await res.blob();
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = currentTicket + ".zip";
  a.click();
  URL.revokeObjectURL(a.href);
});

// ─── Minimal ZIP parser (PKZIP, STORE + DEFLATE) ─────────────────

// Cap on the total decompressed size so a zip bomb cannot OOM the tab.
const ZIP_INFLATE_CAP = 50 * 1024 * 1024;

async function parseZip(buf) {
  const view = new DataView(buf);
  const u8 = new Uint8Array(buf);
  const out = new Map();
  let p = 0;
  let inflated = 0;
  while (p + 4 <= buf.byteLength) {
    const sig = view.getUint32(p, true);
    if (sig !== 0x04034b50) break;
    const flags = view.getUint16(p + 6, true);
    const method = view.getUint16(p + 8, true);
    let compressedSize = view.getUint32(p + 18, true);
    const uncompressedSize = view.getUint32(p + 22, true);
    const nameLen = view.getUint16(p + 26, true);
    const extraLen = view.getUint16(p + 28, true);
    const name = new TextDecoder().decode(u8.subarray(p + 30, p + 30 + nameLen));
    const dataStart = p + 30 + nameLen + extraLen;
    if (flags & 0x08) {
      // Streamed entry: sizes are zero in LFH, find the data descriptor signature.
      let i = dataStart;
      while (i + 4 <= buf.byteLength) {
        if (view.getUint32(i, true) === 0x08074b50) { compressedSize = i - dataStart; break; }
        i++;
      }
    }
    const dataEnd = dataStart + compressedSize;
    const slice = u8.subarray(dataStart, dataEnd);
    let bytes;
    if (method === 0) bytes = slice;
    else if (method === 8) bytes = await inflateCapped(slice, ZIP_INFLATE_CAP - inflated);
    else throw new Error("unsupported compression method " + method);
    inflated += bytes.byteLength;
    if (inflated > ZIP_INFLATE_CAP) throw zipTooBigError();
    if (uncompressedSize && bytes.byteLength !== uncompressedSize) {
      // size mismatch is non-fatal for display; keep what we got
    }
    out.set(name, bytes);
    let next = dataEnd;
    if (flags & 0x08) next += 16; // skip data descriptor (sig + crc + 2 sizes)
    p = next;
  }
  return out;
}

function zipTooBigError() {
  return new Error("zip contents exceed " + (ZIP_INFLATE_CAP / 1024 / 1024) + " MB decompressed, refusing to display");
}

// Inflate a deflate-raw slice, aborting once the output exceeds cap bytes.
async function inflateCapped(slice, cap) {
  const stream = new Response(slice).body.pipeThrough(new DecompressionStream("deflate-raw"));
  const reader = stream.getReader();
  const chunks = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > cap) {
      await reader.cancel();
      throw zipTooBigError();
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(total);
  let off = 0;
  for (const c of chunks) { bytes.set(c, off); off += c.byteLength; }
  return bytes;
}

// Bootstrap
try { getToken(); loadRows(); } catch (e) { document.getElementById("status").textContent = "no token"; }
</script>
</body>
</html>`;

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
  // 10 chars Crockford base32 (no I, L, O, U).
  const alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
  const rnd = crypto.getRandomValues(new Uint8Array(10));
  let out = "";
  for (let i = 0; i < 10; i++) out += alphabet[rnd[i]! % 32];
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

// Parses a JSON body with a size cap. Returns a 413 Response when the body
// exceeds maxBytes, null when the JSON is invalid, the payload otherwise.
async function readJsonCapped<T>(request: Request, maxBytes: number): Promise<T | Response | null> {
  const lenHeader = request.headers.get("Content-Length");
  if (lenHeader && parseInt(lenHeader, 10) > maxBytes) {
    return json({ error: "payload_too_large", max: maxBytes }, 413);
  }
  let text: string;
  try {
    text = await request.text();
  } catch {
    return null;
  }
  if (text.length > maxBytes) return json({ error: "payload_too_large", max: maxBytes }, 413);
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
