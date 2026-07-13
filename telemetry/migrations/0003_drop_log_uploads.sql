-- Drops the log upload index. The app no longer uploads log files: logs stay on
-- the user's machine and are shared by hand when they choose to report a bug.
--
-- The R2 bucket (accshift-logs) held the zips themselves and is not managed by
-- D1, so it is deleted out of band:
--   wrangler r2 bucket delete accshift-logs

DROP INDEX IF EXISTS idx_uploads_date;
DROP TABLE IF EXISTS log_uploads;

-- The budget counters are keyed by endpoint; /logs no longer exists.
DELETE FROM global_budget WHERE endpoint = '/logs';
