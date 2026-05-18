# accshift-telemetry

Cloudflare Worker that collects anonymous Accshift telemetry and accepts
manually uploaded log bundles.

Open source so anyone can verify what happens to data sent by the app.

## What the Worker does

- `POST /track` accepts batches of anonymous events (Mode A) or pseudonymous
  events (Mode B, opt-in)
- `POST /logs` accepts a log zip uploaded manually by the user
- `POST /forget` deletes data tied to an `install_id` (Mode B, GDPR art. 17)
- `POST /export` exports data tied to an `install_id` (Mode B, GDPR art. 20)
- `POST /admin/query` runs SELECT-only D1 queries with a bearer token,
  intended for an external dashboard

Details and legal basis live in `docs/LIA.md` and `docs/privacy-policy-draft.md`
in the parent repository.

## Stack

- Cloudflare Workers
- D1 (long-term aggregates, pseudonyms)
- Analytics Engine (high-cardinality events, 90 days)
- R2 (log zip storage)
- Resend (email alerts on rate limit / budget saturation)

The Worker stays inside the Cloudflare and Resend free tiers at Accshift's scale.

## Privacy guarantees

- IP addresses are never stored. Read in memory to derive `country` and
  `daily_visitor_hash`, then dropped.
- No persistent client identifier is stored on the server in Mode A.
- Anti-abuse rate limiting masks IPs (/24 v4, /48 v6) in alert emails.

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

# R2 bucket. R2 must be enabled in the dashboard first.
npx wrangler r2 bucket create accshift-logs --location weur

# Analytics Engine: enable in the dashboard, the dataset is created
# automatically on first write.

# Apply the D1 schema.
npx wrangler d1 execute accshift-telemetry --file=./schema.sql --remote
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

- `wrangler.toml`: replace `accshift.mtsu.dev` with your own domain (or
  delete the `routes` block to fall back to the workers.dev URL).
- `wrangler.toml`: replace `database_id` with the one created above.
- `wrangler.toml`: replace `ALERT_FROM` with a Resend-verified sender to
  exit sandbox mode.

### Deploy

```bash
npx wrangler deploy
```

### Local dev

```bash
npx wrangler dev
```

Note: local dev does not have access to production secrets. Use `.dev.vars`
for dev-time secrets (gitignored).

## Limits

- Analytics Engine retention is capped at 90 days (Cloudflare technical limit).
- Analytics Engine does not support row-level deletion. The `forget list`
  in D1 is consulted at query time as a workaround.
- Rate limiting is locked to a 60-second window (Cloudflare granularity).

## Licence

Same licence as the parent Accshift repository.
