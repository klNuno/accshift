#!/bin/sh
# Monthly aggregate snapshot of the accshift usage stats.
#
# Why this exists: PostHog's free plan keeps raw events for one year. Every
# chart is recomputed from those raw events, so nothing survives the window.
# This job pulls a handful of counters once a month and appends them to a file
# on a machine we own, which is what makes a multi-year trend possible at all.
#
# Nothing here is personal data. Every value is a count or a country/version
# label aggregated over a whole month, so the file can be kept indefinitely
# without touching the retention promise made in docs/analytics.md.
#
# Deployment (micronist, 192.168.1.50):
#   install -m 755 monthly-stats-snapshot.sh /opt/server/scripts/accshift-stats/run.sh
#   mkdir -p /opt/server/data/accshift-stats
#   printf '%s\n' 'POSTHOG_PERSONAL_API_KEY=phx_...' \
#                 'POSTHOG_PROJECT_ID=12345' > /opt/secrets/posthog.env
#   chmod 600 /opt/secrets/posthog.env
#   echo '5 4 1 * * root /opt/server/scripts/accshift-stats/run.sh' > /etc/cron.d/accshift-stats
#
# Mint a DEDICATED PostHog personal key for this, scoped to `query:read` only.
# Do not reuse the Worker's key: that one also carries person:write, and it has
# no business sitting on a second machine.
#
# The output file must NOT live under /var/log on micronist: Armbian keeps that
# in zram and only flushes hourly, so a freeze loses it. /opt/server is real disk.

set -eu

NAME=accshift-stats
SECRETS=/opt/secrets/posthog.env
OUT=/opt/server/data/accshift-stats/monthly.jsonl
LOG=/var/log/${NAME}.log
LOCK=/var/lock/${NAME}.lock
API_HOST=${POSTHOG_API_HOST:-https://eu.posthog.com}

log() { echo "$(date -Is) $*" >>"$LOG"; }

# House rule: always exit 0 so a failure never turns into a cron mail flood.
# The log line is the alerting surface.
die() { log "[FATAL] $*"; exit 0; }

# Serialize against a previous run that is somehow still going.
if command -v flock >/dev/null 2>&1 && [ "${LOCKED:-}" != "1" ]; then
  LOCKED=1 exec flock -n "$LOCK" "$0" "$@"
fi

command -v curl >/dev/null 2>&1 || die "curl missing"
command -v jq   >/dev/null 2>&1 || die "jq missing"

# shellcheck source=/dev/null
. "$SECRETS" 2>/dev/null || die "$SECRETS unreadable"
[ -n "${POSTHOG_PERSONAL_API_KEY:-}" ] || die "POSTHOG_PERSONAL_API_KEY unset"
[ -n "${POSTHOG_PROJECT_ID:-}" ]       || die "POSTHOG_PROJECT_ID unset"

# The month that just ended, so the window is always complete. Running on the
# 1st means "last month" is closed and will never gain more events.
MONTH=$(date -u -d "last month" +%Y-%m) || die "date arithmetic failed"

WINDOW="timestamp >= toStartOfMonth(now() - INTERVAL 1 MONTH) AND timestamp < toStartOfMonth(now())"

# Runs one HogQL query, echoes the raw `results` array.
# PostHog throttles HogQL to 120/hour across the whole team; five a month is
# nothing, but a failure here still must not abort the whole run silently.
query() {
  body=$(jq -n --arg q "$1" --arg n "$2" '{query:{kind:"HogQLQuery",query:$q},name:$n}')
  out=$(curl -sS --max-time 60 \
    -H "Authorization: Bearer ${POSTHOG_PERSONAL_API_KEY}" \
    -H "Content-Type: application/json" \
    -X POST "${API_HOST}/api/projects/${POSTHOG_PROJECT_ID}/query/" \
    -d "$body") || return 1
  echo "$out" | jq -e '.results' 2>/dev/null || {
    log "[WARN] query $2 returned no results: $(echo "$out" | head -c 300)"
    return 1
  }
}

totals=$(query "SELECT count(DISTINCT distinct_id), count() FROM events WHERE ${WINDOW}" \
  "${NAME}-totals") || die "totals query failed"

dau=$(query "SELECT round(avg(d), 1) FROM (SELECT toDate(timestamp) AS day, count(DISTINCT distinct_id) AS d FROM events WHERE ${WINDOW} GROUP BY day)" \
  "${NAME}-dau") || die "dau query failed"

countries=$(query "SELECT properties.country, count(DISTINCT distinct_id) AS n FROM events WHERE ${WINDOW} GROUP BY properties.country ORDER BY n DESC LIMIT 30" \
  "${NAME}-countries") || die "countries query failed"

versions=$(query "SELECT properties.app_version, count(DISTINCT distinct_id) AS n FROM events WHERE ${WINDOW} GROUP BY properties.app_version ORDER BY n DESC LIMIT 30" \
  "${NAME}-versions") || die "versions query failed"

platforms=$(query "SELECT properties.platform, count() AS n FROM events WHERE event = 'platform_switch' AND ${WINDOW} GROUP BY properties.platform ORDER BY n DESC LIMIT 30" \
  "${NAME}-platforms") || die "platforms query failed"

# `pairs` turns [[label, n], ...] into {label: n}, dropping empty labels so a
# missing property does not create a "" bucket.
line=$(jq -cn \
  --argjson totals "$totals" \
  --argjson dau "$dau" \
  --argjson countries "$countries" \
  --argjson versions "$versions" \
  --argjson platforms "$platforms" \
  --arg month "$MONTH" '
  def pairs: map(select(.[0] != null and .[0] != "")) | map({key: (.[0]|tostring), value: .[1]}) | from_entries;
  {
    month: $month,
    unique_installs: ($totals[0][0] // 0),
    events_total:    ($totals[0][1] // 0),
    dau_avg:         ($dau[0][0] // 0),
    countries: ($countries | pairs),
    versions:  ($versions  | pairs),
    platform_switches: ($platforms | pairs)
  }') || die "jq assembly failed"

mkdir -p "$(dirname "$OUT")" || die "cannot create $(dirname "$OUT")"

# Idempotent: re-running for a month already recorded replaces nothing and adds
# nothing, so a manual retry after a failure is always safe.
if [ -f "$OUT" ] && grep -q "\"month\":\"${MONTH}\"" "$OUT"; then
  log "[SKIP] ${MONTH} already recorded"
  exit 0
fi

echo "$line" >>"$OUT" || die "cannot append to $OUT"
log "[OK] ${MONTH} installs=$(echo "$line" | jq -r .unique_installs) dau=$(echo "$line" | jq -r .dau_avg)"
exit 0
