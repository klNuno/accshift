-- Aggregate onboarding choices, one counter per (date, version, choice).
-- No client or request identifier is stored, including for refusals.
--
-- /consent 500s without this table, so the Worker must never be deployed ahead
-- of it. `pnpm deploy` applies pending migrations first for exactly that reason.

CREATE TABLE IF NOT EXISTS consent_choice_counts (
  date        TEXT NOT NULL,
  app_version TEXT NOT NULL,
  choice      TEXT NOT NULL CHECK (choice IN ('refused', 'basic', 'enhanced')),
  count       INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (date, app_version, choice)
);

CREATE INDEX IF NOT EXISTS idx_consent_choice ON consent_choice_counts(choice, date);
