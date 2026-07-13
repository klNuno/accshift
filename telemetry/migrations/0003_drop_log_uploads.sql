-- Drops the log upload index. The app no longer uploads log files: logs stay on
-- the user's machine and are shared by hand when they choose to report a bug.
-- Apply once on the live D1 database:
--   wrangler d1 execute accshift-telemetry --remote --file=migrations/0003_drop_log_uploads.sql
--
-- The R2 bucket (accshift-logs) holds the zips themselves and is not managed by
-- D1. Delete it separately once this migration is applied:
--   wrangler r2 bucket delete accshift-logs

DROP INDEX IF EXISTS idx_uploads_date;
DROP TABLE IF EXISTS log_uploads;

-- The budget counters are keyed by endpoint; /logs no longer exists.
DELETE FROM global_budget WHERE endpoint = '/logs';
