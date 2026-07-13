-- Accshift telemetry: base D1 schema.
--
-- D1 holds long-term aggregates (beyond the 90-day Analytics Engine retention)
-- and backs the Mode B GDPR operations (forget / export).
--
-- Every statement is idempotent, so replaying this file on a database that was
-- bootstrapped before migrations were tracked is a no-op.

-- ─────────────────────────────────────────────────────────────
-- Daily pings (Mode A + Mode B)
-- ─────────────────────────────────────────────────────────────
-- Mode A: identifier = stable HMAC of a random local UUID for new clients;
-- legacy clients fall back to daily_visitor_hash.
-- Mode B: identifier = install_id (stable while user is opted in)
CREATE TABLE IF NOT EXISTS daily_pings (
  identifier    TEXT NOT NULL,
  is_persistent INTEGER NOT NULL,         -- 0 = Mode A, 1 = Mode B
  date          TEXT NOT NULL,            -- YYYY-MM-DD
  app_version   TEXT NOT NULL,
  os_version    TEXT NOT NULL,
  locale        TEXT,
  country       TEXT,
  PRIMARY KEY (identifier, date)
);

CREATE INDEX IF NOT EXISTS idx_pings_date     ON daily_pings(date);
CREATE INDEX IF NOT EXISTS idx_pings_version  ON daily_pings(app_version, date);
CREATE INDEX IF NOT EXISTS idx_pings_country  ON daily_pings(country, date);

-- ─────────────────────────────────────────────────────────────
-- Accounts snapshot per platform (Mode B only)
-- ─────────────────────────────────────────────────────────────
-- Mode A drops accounts_snapshot events before upload because computing a
-- per-user distribution requires a Mode B install_id.
CREATE TABLE IF NOT EXISTS accounts_snapshot (
  install_id    TEXT NOT NULL,
  date          TEXT NOT NULL,
  platform      TEXT NOT NULL,
  count         INTEGER NOT NULL,
  PRIMARY KEY (install_id, date, platform)
);

CREATE INDEX IF NOT EXISTS idx_snap_platform_date ON accounts_snapshot(platform, date);

-- ─────────────────────────────────────────────────────────────
-- Forget list (Mode B, GDPR art. 17 right to erasure)
-- ─────────────────────────────────────────────────────────────
-- AE does not support row-level deletion, so we keep a list of install_ids
-- to filter out at admin query time.
CREATE TABLE IF NOT EXISTS forgotten (
  install_id TEXT PRIMARY KEY,
  forgotten_at INTEGER NOT NULL       -- unix seconds
);

-- ─────────────────────────────────────────────────────────────
-- Global daily budget: hard cost cap
-- ─────────────────────────────────────────────────────────────
-- Last line of defense: even if all per-IP rate limiters are bypassed
-- (distributed botnet), these counters bound the spend. When the cap is
-- reached the Worker returns 503 Service Unavailable until UTC midnight.
CREATE TABLE IF NOT EXISTS global_budget (
  date     TEXT NOT NULL,
  endpoint TEXT NOT NULL,
  count    INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (date, endpoint)
);
