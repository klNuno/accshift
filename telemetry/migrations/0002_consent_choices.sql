-- Apply with:
--   wrangler d1 execute accshift-telemetry --remote --file=migrations/0002_consent_choices.sql

CREATE TABLE IF NOT EXISTS consent_choice_counts (
  date        TEXT NOT NULL,
  app_version TEXT NOT NULL,
  choice      TEXT NOT NULL CHECK (choice IN ('refused', 'basic', 'enhanced')),
  count       INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (date, app_version, choice)
);

CREATE INDEX IF NOT EXISTS idx_consent_choice ON consent_choice_counts(choice, date);
