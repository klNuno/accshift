# Anonymous analytics

I don't want your data. I want to know whether anyone uses this thing and what
to fix first. Accshift counts a few anonymous things and nothing else, and you
can switch even those off.

No feature is gated on it. There is no nag screen. The app is identical either
way. If you would rather read code than prose, skip to
[verify all of this yourself](#verify-all-of-this-yourself), which is the point
of this page existing in a git repository.

## Turning it off

Settings, Privacy. Two switches, both off means nothing is ever sent again.

Nothing at all is sent before you finish the first-launch screen. After it, the
anonymous counters are on: that screen asks about the enhanced tier, not about
the counters, so turning those off is a separate and deliberate action. Said
plainly, without dressing it up: **the anonymous tier is opt-out, the enhanced
tier is opt-in.**

The `accshift` command-line binary reads the same two switches. It reports one
event per command, it never sends a daily ping (a command you run five times is
one person, not five), and with both switches off it starts no reporting at
all.

## What is never collected

Accshift manages game accounts, so this list matters more than the one below it.
None of the following ever leaves your machine, in any mode:

- Account names, usernames, display names, nicknames
- Platform account identifiers of any kind (SteamID, Riot PUUID, Epic account id)
- Passwords, tokens, cookies, session files, or anything derived from them
- Persona names, folder names, custom labels, avatars
- File paths, directory names, screenshots, window contents
- Your IP address, which is never stored anywhere
- Log files. Logs stay on your machine and are only ever shared by hand, when
  you choose to attach one to a bug report

An event says "an account was added on Steam". It cannot say which account,
because the app never puts that in the payload. The Rust code that builds the
payload is one function, and it is linked at the bottom of this page.

## What is collected

Nineteen events, and that is the complete list.

| Event                     | When                                     | Fields beyond the common ones                                 |
| ------------------------- | ---------------------------------------- | ------------------------------------------------------------- |
| `ping`                    | Once a day while the app runs            | `dropped_events`, only if any were                            |
| `first_run`               | The first launch of an installation      | none                                                          |
| `app_launched`            | Startup finished                         | `duration_ms`                                                 |
| `platform_switch`         | You switched an account                  | `platform`, `duration_ms`, `success`, `error_code` on failure |
| `persona_switch`          | You activated a persona                  | number of platforms, number that succeeded                    |
| `account_add_started`     | You opened an add-account flow           | `platform`                                                    |
| `account_add_cancelled`   | You closed one without adding an account | `platform`                                                    |
| `account_added`           | You finished adding an account           | `platform`                                                    |
| `operation_failed`        | A named operation failed                 | `operation`, `error_code`, `platform`                         |
| `update_available`        | An update was found                      | `target_version`                                              |
| `update_downloaded`       | It finished downloading                  | `target_version`                                              |
| `update_applied`          | It was installed                         | `target_version`                                              |
| `update_failed`           | Any of the three above failed            | `target_version`, `error_code`                                |
| `cli_command`             | A CLI command finished                   | `command`, `success`, `error_code`                            |
| `streamer_mode_activated` | Streamer mode auto-enabled               | none                                                          |
| `deep_link_used`          | An `accshift://` link was opened         | none                                                          |
| `session_ended`           | You closed the window                    | `duration_ms`                                                 |
| `accounts_snapshot`       | Each app start, enhanced only            | `platform`, how many accounts on it                           |
| `settings_snapshot`       | Each app start, enhanced only            | the settings listed below                                     |

One more event exists and is not in that table because it is not tied to an
installation at all: `consent_choice`, recorded once when you answer the
first-launch screen. It carries the answer and the app version, and lands on a
single shared counter so the three possible answers have a denominator.

Every event also carries seven common fields: the app version, a fixed OS
identifier (`windows`, `macos`, `linux`), the architecture, the OS version,
your locale (for example `fr-FR`), whether it came from the app or the CLI, and
the time it happened, to the second. The server adds one more, the country
code, derived from your IP address without storing the address itself.

`platform` is always a fixed identifier like `steam` or `riot`, never anything
you typed. So are `error_code`, `operation` and `command`: each is matched
against a fixed list in the code, and anything unrecognised is recorded as
`other` rather than sent as it was. That is what makes it impossible for an
error message, which routinely contains a file path with your username in it,
to travel inside one of those fields.

`accounts_snapshot` counts how many accounts exist per platform, and nothing
else about your library. `settings_snapshot` reports your interface language,
which platforms are enabled, and whether personas, the PIN lock, the CLI and
deep links are on. No theme name, because a custom theme is one you named
yourself. Both are sent in enhanced mode only: the app drops them before upload
in anonymous mode. Nine low-entropy settings together are a weak fingerprint,
and the anonymous tier exists precisely so that two events cannot be tied to
one installation across days.

Every release up to and including 1.0.2 skipped Steam in `accounts_snapshot`,
because the list it was built from only covered platforms whose accounts live
in the config file. Snapshots recorded by those releases say nothing about
Steam, in either direction.

`first_run` means "first launch that knew how to report one", so for an
installation that predates the release introducing it, it fires on the first
launch after the update rather than on the day it was installed.

This table is checked against the code on every release. Where this page and the
code disagree, the code is right and the page is a bug worth reporting.

## What the payload actually looks like

This is a real batch, exactly as it leaves the machine in anonymous mode:

```json
{
  "mode": "A",
  "anonymous_id": "b3f1c2d4-5a6b-4c7d-8e9f-0a1b2c3d4e5f",
  "events": [
    {
      "name": "ping",
      "app_version": "1.4.2",
      "os": "windows",
      "arch": "x86_64",
      "os_version": "Windows 11 Pro 26200",
      "surface": "gui",
      "client_ts": "2026-08-04T12:34:56Z",
      "locale": "fr-FR"
    },
    {
      "name": "platform_switch",
      "app_version": "1.4.2",
      "os": "windows",
      "arch": "x86_64",
      "os_version": "Windows 11 Pro 26200",
      "surface": "gui",
      "client_ts": "2026-08-04T12:36:02Z",
      "locale": "fr-FR",
      "platform": "steam",
      "duration_ms": 842,
      "success": true,
      "count": 1
    }
  ]
}
```

That is the whole thing. `count` duplicates `success` here for one boring
reason: it carried the success flag before `success` existed, and the
dashboards built against it still read it.

Batches are sent at most once every five minutes, the first one about twenty
seconds after launch, and are held in memory only: telemetry is never written
to your disk, so an app that never reaches the network simply forgets its
events. A batch that fails to send is kept in memory and retried, with the
delay doubling up to an hour; at most 200 events are held that way, and the
oldest are discarded past that. `ping` reports how many were lost, which is the
only reason that count exists.

## The two switches

One aggregate counter records which answer the first-launch screen got, so I
know how many people accept the enhanced tier. It carries no identifier.

### Anonymous counters

Events are sent, and they are deliberately made hard to link together.

A random UUID is generated on your machine. The server never stores it: it
stores a keyed hash of it, used only to avoid counting the same installation
twice in the daily active count. Every other event is attributed to a **different
hash that changes every night**, derived from your IP address and User-Agent. Two
switches you make on Monday and Tuesday cannot be tied to the same installation.

No user profile is created on the analytics side. This mode exists so the
project can answer "how many people use this, on what OS, in what country" and
nothing more.

### Enhanced

A second random UUID, the install id, is generated and attached to every event.
It stays the same over time, which is exactly the point: it makes it possible to
see whether people come back after a week, which features get used together, and
how many accounts a real library holds.

This is the mode where a profile does exist on the analytics side. It is a
separate opt-in for that reason, and it is the only mode where export and
deletion are possible, because they need an identifier to act on.

## Where the data goes

1. Your machine sends the batch to a Cloudflare Worker, which is open source and
   lives in [`server/`](../server) in this repository.
2. The Worker derives the country code from your IP address, computes the
   anonymous-mode hashes, and **discards the address**. It stores nothing itself:
   it has no database.
3. It forwards the events to PostHog, in their EU region, hosted in Germany.
   Every forwarded event explicitly overrides the IP field and disables location
   lookup, so PostHog stores no address and infers no location beyond the
   country code computed in step 2.

Three companies are involved: Cloudflare processes the traffic in transit,
PostHog Inc. stores the events in the EU, and Resend delivers operational alert
emails to the maintainer. Resend never receives event data.

## Retention

Events older than 12 months are deleted.

Once a month, a handful of totals are copied to a machine the maintainer owns,
so the project can see a multi-year trend without keeping any raw event. Those
totals contain no identifier of any kind: every value is a count, or a country,
version or platform label, summed over a whole month. The script that does it is
[`scripts/monthly-stats-snapshot.sh`](../scripts/monthly-stats-snapshot.sh), and
the five queries it runs are the whole of what is kept long term.

## Your controls

Everything below is in Settings, Privacy.

- **Change your mind.** Both switches can be flipped at any time, in either
  direction. Turning everything off stops all sending immediately.
- **Export your data** (enhanced mode). Copies everything held against your
  install id to the clipboard as JSON. Anonymous-mode events cannot be exported
  because there is no identifier to look them up by, which is the point of that
  mode.
- **Delete your data** (enhanced mode). Turning enhanced mode off asks the server
  to delete everything tied to your install id, and the app keeps retrying until
  the server confirms. Your profile and its properties are removed immediately;
  the events themselves are queued for a batch deletion job that the analytics
  provider runs during off-peak hours, so allow up to a week for that part.

The enhanced tier runs on your explicit consent, GDPR article 6(1)(a). Export is
article 20, deletion is article 17. Withdrawing is one click and never degrades
the app.

## Verify all of this yourself

Nothing here has to be taken on trust. The code that decides what is sent is
small and self-contained:

| What                                        | Where                                                                                                           |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| The event list and every field              | [`crates/accshift-core/src/telemetry/events.rs`](../crates/accshift-core/src/telemetry/events.rs)               |
| The fixed vocabularies codes are matched to | [`crates/accshift-core/src/telemetry/events.rs`](../crates/accshift-core/src/telemetry/events.rs)               |
| The exact payload built for the network     | [`crates/accshift-core/src/telemetry/client.rs`](../crates/accshift-core/src/telemetry/client.rs)               |
| The queue, and why it never touches disk    | [`crates/accshift-core/src/telemetry/queue.rs`](../crates/accshift-core/src/telemetry/queue.rs)                 |
| The consent gate                            | [`crates/accshift-core/src/telemetry/mod.rs`](../crates/accshift-core/src/telemetry/mod.rs)                     |
| What the OS fields are read from            | [`crates/accshift-core/src/telemetry/platform_info.rs`](../crates/accshift-core/src/telemetry/platform_info.rs) |
| The CLI's own reporting                     | [`crates/accshift-cli/src/telemetry.rs`](../crates/accshift-cli/src/telemetry.rs)                               |
| The server, in full                         | [`server/src/index.ts`](../server/src/index.ts)                                                                 |
| The server's own README                     | [`server/README.md`](../server/README.md)                                                                       |

This page is versioned alongside the code it describes, so `git log` on it shows
every change ever made to what gets collected.
