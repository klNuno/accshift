# accshift-telemetry

Cloudflare Worker that collects optional accshift usage telemetry.

Open source so anyone can verify what happens to data sent by the app.

## What the Worker does

- `POST /track` accepts Mode A or Mode B event batches; their identifier
  behavior is detailed below
- `POST /consent` increments one of three aggregate onboarding-choice counters
- `POST /forget` deletes D1 rows tied to an `install_id`; Analytics Engine
  residual events expire within 90 days (Mode B, GDPR art. 17)
- `POST /export` exports the D1 rows tied to an `install_id`; Analytics Engine
  events are not included (Mode B, GDPR art. 20)
- `POST /admin/query` runs SELECT-only D1 queries with a bearer token,
  intended for an external dashboard

See the public [telemetry and privacy documentation](https://github.com/klNuno/accshift/wiki/Telemetry)
for the fields, retention periods, legal basis, and data-subject rights.

## Stack

- Cloudflare Workers
- D1 (long-term aggregates, pseudonyms)
- Analytics Engine (high-cardinality events, 90 days)
- Resend (email alerts on rate limit / budget saturation)

## Data handling

- Raw IP addresses are not written to D1 or Analytics Engine. They are
  processed in memory to derive `country` and, for Mode A events, a daily HMAC
  of the IP address and User-Agent; Cloudflare also uses the IP as a 60-second
  rate-limit key.
- Mode A keeps a random UUID locally. The Worker stores only a
  purpose-specific HMAC for unique-installation pings; regular usage events
  still use a hash that rotates daily.
- Country is stored with telemetry events and daily pings.
- Onboarding choices store no identifier at all. Even a refusal increments
  only a date/version/choice aggregate.
- Anti-abuse rate limiting masks IPs (/24 v4, /48 v6) in alert emails.
- Cloudflare processes telemetry and hosts D1/Analytics Engine. Resend receives
  operational alert emails only, including a masked IP prefix for rate-limit
  alerts, never event payloads.

## Configuration (forking)

### Prerequisites

- A Cloudflare account
- `wrangler` CLI installed and logged in (`pnpm install` then
  `npx wrangler login`)
- A Resend account for alerts (optional)

### Resource creation

```bash
# D1 database. Take the returned id and put it in wrangler.toml.
npx wrangler d1 create accshift-telemetry

# Analytics Engine: enable in the dashboard, the dataset is created
# automatically on first write.
```

The schema is not applied by hand. `migrations/` is the single source of truth
and wrangler tracks what it has run in a `d1_migrations` table, so a fresh
database and a years-old one converge on the same schema:

```bash
pnpm db:status    # what is pending
pnpm db:migrate   # apply it
```

`pnpm deploy` runs the pending migrations _before_ uploading the Worker. That
ordering is the point: it is what stops code from reaching production ahead of a
table it queries. Use `pnpm deploy:worker-only` to skip it when you know the
schema has not moved.

Choice percentages can be queried through `/admin/query` with:

```sql
SELECT choice,
       SUM(count) AS responses,
       ROUND(100.0 * SUM(count) / (SELECT SUM(count) FROM consent_choice_counts), 1) AS percentage
FROM consent_choice_counts
GROUP BY choice
ORDER BY choice;
```

### Secrets

```bash
# Random 32-byte hex.
node -e "console.log(require('crypto').randomBytes(32).toString('hex'))" | npx wrangler secret put HASH_SECRET
node -e "console.log(require('crypto').randomBytes(32).toString('hex'))" | npx wrangler secret put ADMIN_TOKEN

# Resend (for rate limit alerts, optional).
echo "re_xxxxxxxx" | npx wrangler secret put RESEND_API_KEY
echo "you@example.com" | npx wrangler secret put ALERT_EMAIL
```

### Customisations

- `wrangler.toml`: replace the custom-domain pattern in `routes` with your own,
  or delete the block to use the workers.dev URL.
- `wrangler.toml`: replace `database_id` with the one created above.
- `wrangler.toml`: replace `ALERT_FROM` with a Resend-verified sender to
  exit sandbox mode.

### Deploy

```bash
pnpm deploy
```

Not `wrangler deploy` directly: that uploads the Worker without applying the
pending migrations, which is how `/consent` once shipped against a table that
did not exist.

### Local dev

```bash
npx wrangler dev
```

Note: local dev does not have access to production secrets. Use `.dev.vars`
for dev-time secrets (gitignored).

## Limits

- Analytics Engine retention is capped at 90 days (Cloudflare technical limit).
- Analytics Engine does not support row-level deletion. The `forget list`
  in D1 records the opt-out while residual events expire within 90 days.
- `/export` returns the install-scoped D1 rows (`daily_pings` and
  `accounts_snapshot`) only. High-frequency Analytics Engine events cannot be
  exported per installation.
- Rate limiting is locked to a 60-second window (Cloudflare granularity).

## Licence

Same licence as the parent Accshift repository.
