# Server

Cloudflare Worker that receives the optional anonymous usage counters described
in [docs/analytics.md](../docs/analytics.md). Deployed under the Worker name
`accshift-telemetry`, which is what the app's compiled endpoint points at;
renaming it would orphan the live deployment, so the directory moved and the
Worker name did not.

Open source so anyone can verify what happens to data sent by the app.

Analytics are stored in [PostHog](https://posthog.com) EU Cloud. This Worker is
the privacy layer in front of it: it stores nothing itself, strips the client IP
before anything leaves Cloudflare, and holds the API key that makes deletion and
export possible.

## What the Worker does

- `POST /track` accepts Mode A or Mode B event batches and forwards them to
  PostHog; their identifier behavior is detailed below
- `POST /consent` records one aggregate onboarding-choice event carrying no
  installation identifier
- `POST /forget` deletes the PostHog person tied to an `install_id` and queues
  deletion of their events (Mode B, GDPR art. 17)
- `POST /export` returns the PostHog events tied to an `install_id`
  (Mode B, GDPR art. 20)

See the public [telemetry and privacy documentation](../docs/analytics.md) for
the fields, retention periods, legal basis, and data-subject rights. It lives in
the repository rather than the wiki so that every change to what gets collected
shows up in `git log`.

## Stack

- Cloudflare Workers
- PostHog EU Cloud (Frankfurt) for storage, dashboards and retention analysis
- Resend (email alerts on rate limit saturation)

There is no database. The Worker holds no storage binding of any kind.

## Why a proxy instead of talking to PostHog directly

The app could POST to PostHog itself. It does not, for four reasons:

- `/forget` and `/export` need a PostHog personal API key, which can read and
  delete the entire project. A desktop binary is not a secret store, so that key
  has to live server-side or those endpoints cannot exist.
- The Mode A daily hash is derived from the client IP, which the client cannot
  see. Without a server, Mode A would have to fall back to a stable local
  identifier, which is Mode B by another name.
- PostHog never sees a user's IP address. It sees this Worker.
- A released desktop binary is permanent. The Worker is the seam that lets the
  backend change without shipping a new version to people who never update.

## Data handling

- Raw IP addresses are never stored and never leave Cloudflare. They are
  processed in memory to derive `country` and, for Mode A events, a daily HMAC
  of the IP address and User-Agent; Cloudflare also uses the IP as a 60-second
  rate-limit key. The only address PostHog could observe is this Worker's, and
  every forwarded event overwrites `$ip` with `0.0.0.0` and carries
  `$geoip_disable`, so no address is recorded and no location is inferred.

  The placeholder is deliberately truthy. PostHog back-fills `$ip` from the
  request socket whenever the submitted value is falsy, so sending `null` would
  store an address rather than suppress one. Enabling "Discard client IP data"
  on the PostHog project drops the property entirely and is the second layer.

- Mode A keeps a random UUID locally and creates no PostHog person profile
  (`$process_person_profile: false`). Usage events use a hash that rotates
  daily and are therefore unlinkable across days. The single exception is the
  `ping` event, which uses a stable purpose-bound HMAC of the local UUID,
  because unique-installation counts are impossible with an identifier that
  changes every night.
- Mode B uses the `install_id` as the PostHog distinct id. That is what makes
  retention metrics, cohorts and per-installation deletion possible, and it is
  why it is a separate opt-in.
- Country is stored as an event property.
- Onboarding choices store no identifier at all. Even a refusal is recorded
  against a single shared aggregate id, never one tied to an installation.
- Anti-abuse rate limiting masks IPs (/24 v4, /48 v6) in alert emails.
- Cloudflare processes telemetry in transit. PostHog Inc. stores it, in the EU
  region. Resend receives operational alert emails only, including a masked IP
  prefix for rate-limit alerts, never event payloads.

## Configuration (forking)

### Prerequisites

- A Cloudflare account
- `wrangler` CLI installed and logged in (`pnpm install` then
  `npx wrangler login`)
- A PostHog account (EU Cloud recommended, the free tier covers 1M events/month)
- A Resend account for alerts (optional)

### PostHog setup

1. Create a project. Its numeric id is in the dashboard URL; put it in
   `POSTHOG_PROJECT_ID` in `wrangler.toml`.
2. Copy the project API key (`phc_...`) from project settings. It is used for
   ingestion only.
3. Create a personal API key (`phx_...`) scoped to `person:read`,
   `person:write` and `query:read`. Nothing more: this key is what `/forget`
   and `/export` use, and a wider scope buys nothing.
4. Turn on "Discard client IP data" in Settings, Project, General. The Worker
   already overwrites `$ip`, this removes the property outright.
5. Set a billing limit on the project. It is the only hard spend ceiling; the
   Worker's rate limiters bound burst rate, not a monthly total.

### Secrets

```bash
# Random 32-byte hex.
node -e "console.log(require('crypto').randomBytes(32).toString('hex'))" | npx wrangler secret put HASH_SECRET

# PostHog.
echo "phc_xxxxxxxx" | npx wrangler secret put POSTHOG_PROJECT_API_KEY
echo "phx_xxxxxxxx" | npx wrangler secret put POSTHOG_PERSONAL_API_KEY

# Resend (for rate limit alerts, optional).
echo "re_xxxxxxxx" | npx wrangler secret put RESEND_API_KEY
echo "you@example.com" | npx wrangler secret put ALERT_EMAIL
```

Rotating `HASH_SECRET` is not a neutral operation: it changes every Mode A
identifier, so unique-installation counts restart from zero and past Mode A
events become unreachable from new ones. That is a privacy property, not a bug,
but do not do it casually.

### Customisations

- `wrangler.toml`: replace the custom-domain pattern in `routes` with your own,
  or delete the block to use the workers.dev URL.
- `wrangler.toml`: set `POSTHOG_PROJECT_ID`.
- `wrangler.toml`: switch `POSTHOG_INGEST_HOST` and `POSTHOG_API_HOST` to the
  US hosts (`https://us.i.posthog.com`, `https://us.posthog.com`) if your
  project is not in the EU region. Getting this wrong fails silently at
  ingestion: the key is region-scoped.
- `wrangler.toml`: replace `ALERT_FROM` with a Resend-verified sender to
  exit sandbox mode.

### Deploy

```bash
pnpm deploy
```

### Local dev

```bash
npx wrangler dev
```

Note: local dev does not have access to production secrets. Use `.dev.vars`
for dev-time secrets (gitignored). Point them at a throwaway PostHog project
rather than the production one, since `wrangler dev` writes real events.

## Limits

- Event deletion is asynchronous. `/forget` returns once PostHog has accepted
  the request (202): the person record and its properties go immediately, the
  events are queued for a batch job that PostHog runs during off-peak hours,
  weekly on Cloud. "Deleted" therefore means "irreversibly scheduled", not
  "already gone", and the public privacy documentation says so.
- That job only sweeps events captured **before** the request. The app closes
  its queue before calling `/forget`, so the residual window is a batch already
  on the wire, but it is not zero. A second `/forget` clears any residue.
- `/export` covers Mode B only. Mode A events carry no `install_id` and cannot
  be attributed to an installation, which is the entire point of that mode.
- `/export` is capped at 10000 events per installation.
- `/export` runs a HogQL query, and PostHog throttles those to 120 per hour
  across the whole team, not per key. It is a rare user-triggered action so the
  ceiling is theoretical, but a burst of exports surfaces as 502 to the client.
- Rate limiting is locked to a 60-second window (Cloudflare granularity).
- The global limiter is a burst ceiling keyed on a constant, not a daily
  budget. The hard spend stop is the PostHog project billing limit.
- A 200 from PostHog's ingestion endpoint means the batch was accepted, not
  that every event in it was valid. Ingestion errors are not visible here.
- A `/track` batch is rejected with 502 when PostHog is unreachable. The client
  drops its in-memory buffer on error and does not retry, so those events are
  lost rather than queued. This is deliberate: nothing about telemetry is worth
  persisting to a user's disk.

## Licence

Same licence as the parent Accshift repository.
