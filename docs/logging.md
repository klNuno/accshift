# Logging and diagnostics

The log is a diagnostic tool, not a stream of sentences. Every record carries a
code from a closed catalog, a typed payload, the launch it belongs to and, for
anything a user triggered, the operation it belongs to. That is what makes a
failed attempt replayable from a single identifier instead of readable only by
whoever wrote the message.

## Where it lives

The log directory is `logs/` under the app config directory:

| OS      | Path                                                      |
| ------- | --------------------------------------------------------- |
| Windows | `%APPDATA%\com.accshift.desktop\logs`                     |
| Linux   | `~/.config/com.accshift.desktop/logs`                     |
| macOS   | `~/Library/Application Support/com.accshift.desktop/logs` |

Debug builds add a `dev` segment (`logs/dev/`) so development never writes into
the installed app's log.

| File                       | What it is                                               |
| -------------------------- | -------------------------------------------------------- |
| `app.log`                  | Current log, JSONL, one record per line                  |
| `app.1.log` to `app.4.log` | Rotated files, `app.1.log` being the most recent         |
| `app.log.lock`             | Cross-process lock, plus the rotation generation counter |
| `log-levels.json`          | Per-module levels and the temporary debug window         |
| `anomalies.json`           | Failure streaks and duration baselines, across launches  |
| `diagnostic-report.md`     | Last report written by `accshift diag bundle`            |

`app.previous.log` is the legacy name of the single rotated file. It is
migrated into `app.1.log` on the first launch that sees it, then never written
again.

## The record

`docs/log-schema.json` is the generated JSON Schema, and it is the reference.
The summary:

| Column          | Always present | Meaning                                               |
| --------------- | -------------- | ----------------------------------------------------- |
| `schemaVersion` | yes            | `2`. Version 1 is the legacy line described below     |
| `tsMs`          | yes            | Unix milliseconds when the record was built           |
| `level`         | yes            | `trace`, `debug`, `info`, `warn`, `error`             |
| `code`          | yes            | Catalog code. Every possible value is in the catalog  |
| `source`        | yes            | Dotted module, for example `platform.steam`           |
| `runId`         | yes            | `run-<12 hex>`, one per process launch                |
| `fields`        | yes            | Typed payload declared by the code                    |
| `msg`           | no             | Human sentence. Never parse this, parse `fields`      |
| `opId`          | no             | `op-<12 hex>`, shared by every record of one attempt  |
| `durMs`         | no             | Duration, on the record that closes an operation      |
| `outcome`       | no             | `success`, `failure` or `cancelled`                   |
| `errKind`       | no             | Error family, so failures group without parsing `msg` |

One record, as it is actually written, on a single line:

```text
{"schemaVersion":2,"tsMs":1770000000000,"level":"error","code":"platform.switch.failed","source":"platform.steam","runId":"run-4f2a1c9d0e77","opId":"op-91b3ce70aa42","durMs":1840,"outcome":"failure","errKind":"client_running","msg":"Steam refused to exit","fields":{"platform":"steam","step":"kill-launcher"}}
```

Field names never collide with columns: a catalog field called `level` or
`code` would make a filter silently match the wrong thing, so a test rejects
one at build time. That is why the level-change event carries `newLevel` and
the self-report event carries `offendingCode`.

Two synthetic keys can appear inside `fields`: `_defects` lists declaration
violations of the emitting call site, and `_truncatedFields` counts values
dropped or shortened to fit the per-record size budget.

Version 1 records, written by the legacy `append_app_log` facade, have exactly
`tsMs`, `level`, `source`, `message` and `details`. Queries treat them as the
code `legacy.record` so they still show up in a search rather than being
skipped as unparsable.

## Codes

`crates/accshift-core/src/diagnostics/catalog.rs` is the single source of
truth. The `event_catalog!` macro declares, for each code: its default level,
what it means, what to do about it, its required fields with their types, its
optional fields, and any former spelling kept as an alias.

Consequences worth knowing:

- A code that is not in the catalog does not compile. `event()` takes a
  `&'static EventCode`, and those only exist as catalog constants.
- A missing required field, or a field of the wrong type, fails the tests. In
  debug builds it panics at the call site; in release it writes a second record
  under `diagnostics.event.invalid` naming the offending code, so a broken call
  site is findable rather than silent.
- A code is never renamed without an alias. Old spellings keep resolving in
  `--code` filters and in `--explain`, which is what lets a log written by an
  older build stay queryable.

`docs/log-catalog.json` is generated from the catalog and holds the 29 codes
with their meaning, their action and their fields. Regenerate it after touching
the catalog:

```bash
ACCSHIFT_UPDATE_DOCS=1 cargo test -p accshift-core   # regenerate both files
accshift diag schema --write docs                    # same, from a built CLI
cargo test -p accshift-core                          # fails if they drifted
```

The check compares the parsed value, not the bytes, so the repository
formatter stays free to lay those files out as it likes.

The families, in order of how often they matter:

| Prefix                | What it covers                                          |
| --------------------- | ------------------------------------------------------- |
| `op.*`                | Operation opened, stepped, closed                       |
| `platform.*`          | Switch and snapshot outcomes                            |
| `health.*`            | Invariant results: paths, locks, launchers, disk, clock |
| `anomaly.*`           | Patterns visible only across several runs               |
| `log.*`               | Rotation, retention, write failures                     |
| `diagnostics.*`       | Level changes, debug window, report written             |
| `app.session.started` | One per launch, with the environment                    |

## Operations

Anything a user triggers opens an operation. It gets an `opId`, every record
below it carries that same id, and the closing record carries the duration and
the outcome.

```rust
let op = ops::start(&ctx, "platform.switch").platform("steam").trigger("gui").begin();
op.step("kill-launcher");
op.event(&catalog::HEALTH_LAUNCHER_RUNNING)
    .field("platform", "steam")
    .emit(&*op.ctx());
op.fail("client_running", "Steam refused to exit");
```

`with_operation` wraps a `Result`-returning body and closes the operation from
its outcome, mapping the error to the `errKind` vocabulary. An operation
dropped without an explicit verdict closes itself as `cancelled` with a message
saying so, so an early return or a panic still leaves a closing line rather
than a hole.

The GUI's diagnostic check and report actions are traced end to end and return
their `opId` in the answer, which is the intended pattern for a UI: show the
error, quote the id, let the user paste it into
`accshift diag logs --op <id>`. The account switch chain is not instrumented
yet; that migration is deliberately separate from this layer.

## Levels

The default level is `info`. Levels are per module, resolved by longest
matching prefix on `source`, dot-bounded: an override on `platform` covers
`platform.steam`, an override on `platform.steam` wins over it, and neither
touches `platformer`.

```bash
accshift diag level                        # show the current levels
accshift diag level --set debug            # change the default
accshift diag level --module platform.steam --set trace
accshift diag level --module platform.steam --reset
accshift diag level --debug-for 15m        # temporary, reverts on its own
accshift diag level --debug-for 15m --module platform.steam
accshift diag level --stop-debug           # end the window now
```

A temporary window can only make logging more verbose, never quieter, and it is
capped at one hour. It expires on its own: the next launch, or the next level
lookup after the deadline, closes it and writes `diagnostics.debug.expired`.
Nobody has to remember to turn it off, and nobody has to edit a file: the state
lives in `log-levels.json`, written by the commands above.

## Health invariants

Invariants run before a risky action rather than after it fails. Each result is
a code with an action attached, so a reader never has to look it up.

| Check            | Code on failure                 | What it means                                |
| ---------------- | ------------------------------- | -------------------------------------------- |
| Path exists      | `health.path.missing`           | The configured directory or file is gone     |
| Path writable    | `health.path.permission_denied` | It exists but a write probe fails            |
| File unlocked    | `health.file.locked`            | Another process holds it                     |
| Launcher stopped | `health.launcher.running`       | A process that must be closed is alive       |
| Profile parses   | `health.profile.corrupt`        | A JSON file the action depends on is invalid |
| Free space       | `health.disk.low`               | Less than the caller requires is available   |
| Clock sane       | `health.clock.skew`             | System time moved backwards past tolerance   |

A pass writes `health.check.passed` at `debug`, so a green run costs nothing at
the default level but is there when the window is open.

Callers describe what they need and let the report say what holds:

```rust
let report = health::Preflight::new()
    .platform("steam")
    .require_writable_path(&config_dir, "steam config")
    .require_unlocked_file(&loginusers_vdf)
    .require_stopped(&["steam.exe"])
    .require_valid_json(&accounts_json)
    .require_free_space(&config_dir, 8 * 1024 * 1024)
    .run_and_emit(&ctx, Some(op.id()));
if let Some(reason) = report.blocking_reason() {
    return Err(reason);
}
```

The startup report checks the log root is writable, that 32 MiB is free, and
that the clock is sane.

## Anomaly counters

Some problems only exist across runs, so a few counters survive in
`anomalies.json`:

| Code                                    | Trigger                                         |
| --------------------------------------- | ----------------------------------------------- |
| `anomaly.platform.consecutive_failures` | 3 failures in a row on one platform             |
| `anomaly.operation.slow`                | Far above this operation's own rolling baseline |
| `anomaly.snapshot.empty`                | A snapshot captured nothing                     |
| `anomaly.restore.no_write`              | A restore wrote nothing                         |

The slow-operation baseline is a rolling mean and variance over successful runs
only, so an early-aborting failure cannot drag the mean down and make everything
else look slow. It needs 8 samples before it accuses anything, and it requires
both 3 sigma and 1.5 times the mean, with a 750 ms floor, so a fast stable
operation is never reported for a few extra milliseconds.

## Rotation, retention and disk budget

| Setting                       | Value   |
| ----------------------------- | ------- |
| Maximum size of `app.log`     | 2 MiB   |
| Rotated files kept            | 4       |
| Maximum age of a rotated file | 14 days |
| Announced disk budget         | 10 MiB  |

Rotation happens before the write that would breach the cap, never after, so
the cap holds. Files older than the retention window are purged at rotation and
the purge writes `log.retention.purged`.

The CLI and the GUI write the same file, and both take the same exclusive lock
on `app.log.lock`. Since rotation renames the file another process may be
holding open, the first 8 bytes of that lock file hold a generation counter:
a rotating process bumps it, and any peer that finds a newer generation than
its own drops its handle and reopens `app.log` instead of appending into a file
that is now named `app.1.log`.

## What never reaches the log

The redaction applies to messages and, recursively, to every string reachable
from `fields`, including object keys. It removes email addresses, BattleTags,
UUIDs and PUUIDs, and rewrites paths that contain the OS account name into a
placeholder.

A log record never contains a token, a password, a cookie, the contents of a
`.maFile`, or a path still carrying an unredacted system user name. Identifiers
with no stable shape, Steam login names and persona names in particular, are
kept out at the call sites, because any pattern broad enough to catch them
would also eat ordinary words.

Nothing here reaches the network. The diagnostic report is a local file, and
none of this data joins the analytics described in
[analytics.md](./analytics.md).

## Reading the log

```bash
accshift diag logs                              # last 200 records
accshift diag logs --op op-91b3ce70aa42         # one attempt, in order
accshift diag logs --run run-4f2a1c9d0e77       # one launch
accshift diag logs --level warn --since 6h
accshift diag logs --code platform.switch.failed --code anomaly.operation.slow
accshift diag logs --source platform --platform steam --contains vdf
accshift diag logs --all --json > log.jsonl
accshift diag explain platform.switch.failed
accshift diag check
accshift diag bundle
```

Filters combine with AND. `--since` accepts `90s`, `30m`, `6h`, `7d`, and a
bare number is minutes. `--code` resolves aliases, so an old spelling finds
records written under the new one and the reverse. Piping switches the output
to the `accshift.v1` JSON envelope; a terminal gets a table. Rotated files are
searched too, oldest first, and the result reports how many lines were scanned
and how many could not be parsed.

The GUI exposes the same surface through one command, `diagnostics`, taking a
tagged request (`logs`, `summary`, `explain`, `check`, `levels`, `setLevel`,
`startTemporaryDebug`, `stopTemporaryDebug`, `bundle`, `schema`).

## The diagnostic report

```bash
accshift diag bundle                 # writes diagnostic-report.md, says where
accshift diag bundle --print         # also prints it
accshift diag bundle --op op-91b3ce70aa42 --level debug --records 500
accshift diag bundle --no-config     # leave the configuration out entirely
```

One file, pasteable, capped at 256 KiB, holding: app and OS version, the health
invariants, the anomaly counters, log storage state, current levels, a redacted
configuration summary, the codes present with what to do about each, and the
recent log as JSONL. The log tail is the only section that gets cut, and the
cut is announced in the file.

The configuration summary is deny-by-default: any key whose name suggests a
secret becomes `<set>` or `<unset>`, booleans and numbers are kept, and a
string is kept only if its key is on the allow-list, otherwise it becomes
`<redacted len=N>`. A new config field is therefore redacted until someone
decides it is safe, not the reverse.

## Debugging with this

For an agent or a human working from a failure report:

1. Get the `opId` from the user, or find the failure:
   `accshift diag logs --level error --since 24h --json`.
2. Replay the whole attempt in order: `accshift diag logs --op <id> --json`.
   The last `op.step` before the failing record is where it broke, and
   `errKind` on the closing record says which family it belongs to.
3. Ask what the code means and what to do:
   `accshift diag explain <code>` returns the meaning, the action and the
   declared fields.
4. Check whether it is a state problem rather than a code problem:
   `accshift diag check` runs the invariants.
5. Ask whether it repeats: `anomaly.*` codes in
   `accshift diag logs --code anomaly.platform.consecutive_failures`.
6. Need more detail: `accshift diag level --debug-for 15m --module <module>`,
   reproduce, read again. The window closes itself.
7. Everything at once for a report: `accshift diag bundle --print`.

Parse `fields`, never `msg`. Codes and field names are stable, message wording
is not.
