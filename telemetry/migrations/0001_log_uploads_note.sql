-- Adds the user-typed reason attached to a log upload.
-- Apply once on the live D1 database:
--   wrangler d1 execute accshift-telemetry --remote --file=migrations/0001_log_uploads_note.sql

ALTER TABLE log_uploads ADD COLUMN note TEXT;
