# Platform descriptors

Most platforms in accshift are not code. They are a JSON file describing where
a launcher keeps its session, and one generic engine runs it. Adding a platform
therefore needs no compiler and no release: drop a file in a folder and the app
picks it up.

This document is the reference for that file. It covers the fields, how a
descriptor is validated, what it is allowed to touch, and how to try one out
without writing anything to disk.

## Where descriptors come from

There are two sources, and the app tells them apart on screen.

**Shipped.** `crates/accshift-core/src/platforms/descriptor/descriptors/*.json`
is embedded in the binary at build time. These are read-only: a file in the user
folder can never take over one of these ids. The frontend reads the very same
files to build its platform list, so a shipped descriptor describes a platform
once for both sides.

**User.** `<config root>/descriptors/*.json`, next to the custom themes. The
settings screen shows the exact folder, opens it, and reloads it on demand, so a
file added or edited there takes effect with no restart. A platform loaded this
way is labelled as user provided everywhere it appears.

Steam is neither: it stays hand-written Rust. VDF parsing, ban checks, the CS2
bridge and bulk editing are too specific to express as data.

## The shape of a file

```json
{
  "id": "acme",
  "schemaVersion": 1,
  "name": "Acme Launcher",
  "shortName": "Acme",
  "os": {
    "windows": { "...one profile per operating system..." }
  }
}
```

| Field           | Meaning                                                                                     |
| --------------- | ------------------------------------------------------------------------------------------- |
| `id`            | Stable key. Letters, digits, `-` and `_`, and it becomes the file name in the user folder.  |
| `schemaVersion` | `1` today. A descriptor written for another version is refused rather than half-understood. |
| `name`          | Shown in the interface.                                                                     |
| `shortName`     | Used inside messages, where the full name reads badly.                                      |
| `os`            | One profile per system: `windows`, `linux`, `macos`.                                        |

A platform may describe one system, or three, and be complete on some and absent
on others. That is not a broken descriptor: on a system it does not describe, the
platform simply does not appear.

## An OS profile

The shape, with every section empty. It is a map of what follows, not a file to
copy: an empty `identity.source` or `detect` is refused.

```json
{
  "roots": { "files": ["${LOCALAPPDATA}/Acme"], "registry": [] },
  "detect": { "executableResolves": true },
  "executable": { "fileName": "Acme.exe", "candidates": [] },
  "identity": { "source": {}, "format": {}, "current": "identity" },
  "state": { "files": [], "directories": [], "registryValues": [], "caches": [] },
  "close": { "processes": ["Acme.exe"] },
  "launch": {},
  "setup": {}
}
```

### `roots`: the sandbox

Every read and every write the engine performs must land inside one of these
roots. A path that resolves outside them is refused at run time, and a template
containing a `..` segment is refused at load time. `roots.registry` does the same
for registry keys, as `{ "root": "HKCU", "key": "Software\\Acme" }`.

This is the field to get right first. It is the only thing standing between a
descriptor and the rest of the user's disk.

### `detect`: is the launcher here

`executableResolves` is true when the binary below can be found;
`pathExists` lists locations whose presence is enough. Any satisfied condition
means installed. A profile that declares neither is refused: a platform that can
never report itself installed is a mistake, not a choice.

### `executable`: finding the binary

`fileName` is appended when a candidate resolves to a directory. `candidates`
are tried in order and the user's own path override always wins:

| `kind`           | Fields                                                 |
| ---------------- | ------------------------------------------------------ |
| `path`           | `template`                                             |
| `registry`       | `root`, `key`, `value`                                 |
| `uninstallEntry` | `displayName`, `value` (defaults to `InstallLocation`) |

`relativeProbes` covers launchers shipping per-architecture binaries
(`Binaries/Win64`), and `selectFilter` is the filter of the "choose the
executable" dialog.

### `identity`: what an account is

`source` says where the account id comes from:

| `kind`       | What it reads                                                                                                       |
| ------------ | ------------------------------------------------------------------------------------------------------------------- |
| `registry`   | `root`, `key`, `value`.                                                                                             |
| `synthetic`  | Nothing: the launcher exposes no id, so one is minted at capture.                                                   |
| `logTail`    | `path`, `lineContains`, `prefix`, `nearWord`, `tailBytes`. Read most recent line first, with shared access.         |
| `nativeHook` | `name` picks a compiled hook, `paths` gives it the locations it may work on. Allowed only for `riot` and `discord`. |

`format` is `charset` (`digits`, `hex`, `alphanumeric`, `uuid`), `maxLength`,
`minLength`, `lowercase` and an optional `invalidMessage`. Account ids are joined
into snapshot paths, so the charset is a path-traversal guard first and a sanity
check second.

`current` is `identity` (read live every time) or `config` (remembered by us,
for launchers that keep no readable marker). `discovery` widens the account list
with places accounts leave a trace, so accounts added outside accshift still show
up.

### `state`: what is captured and restored

`files`, `directories` and `registryValues` each pair a `live` location with a
`snapshot` name inside the account's encrypted snapshot directory. The flags that
matter:

- `snapshotMarker`: its presence in a snapshot means the account has one.
- `clearOnSetup`: deleted when a setup flow clears the live session.
- `removeLiveBeforeRestore`: for hidden or system files, which cannot be
  truncated in place on Windows.
- `clearSnapshotWhenSourceMissing` (default true): drops a stale snapshot when
  the live file is gone, so a later restore cannot resurrect another account's
  file.

`caches` are wiped after the incoming session is in place and never captured, and
`captureWhen` guards the capture itself: a session the user signed out of by hand
would otherwise overwrite a good snapshot with an empty one.

A `live` value is normally a string. An array means the same thing lives in one
of several places depending on how the launcher was installed: the first that
exists wins, and the first listed is used when none do.

### `close` and `launch`

`close.processes` are shut down before any file is touched, with `timeoutMs` per
process and `settleMs` afterwards so exit-time flushes land. `beforeCapture`
moves the shutdown ahead of the capture, for clients that only write their
session out when they exit.

`launch.args` are passed to the binary, optionally only when the resolved binary
is named `argsOnlyFor`, which is how a launcher reached through an updater stub
gets its hand-off argument without confusing the real client.

### `setup`: adding an account by signing in

`trigger` is polled while the user signs in; once every condition holds the
launcher is closed so it flushes, and `confirm` is re-checked before anything is
captured. Conditions are `newIdentity`, `identityPresent`, `pathNonEmpty`,
`pathFresh`, `anyOf` and `sinceStart`.

`adoptSignedIn` takes the session already on the machine as the new account,
instead of wiping it and asking the user to sign in again.

## Path templates

Locations are written with `${...}` placeholders. `${installDir}` is the
directory holding the launcher binary; every other name is an environment
variable. Both separators are accepted and normalised for the running system, so
one template serves Windows and Linux where the layout allows it.

```
${LOCALAPPDATA}/Acme/session.json
${installDir}/config/user.cfg
```

A template with an unclosed `${`, an empty or oddly spelled placeholder name, or
a `..` segment is refused at load time.

## Validation

A descriptor either loads or is refused with the file, the offending field and
what was expected:

```
Invalid platform descriptor acme.json: field `schemaVersion` expected 1, found 99
Invalid platform descriptor acme.json: field `os.windows.state.files[0].live`
  expected a path that stays inside its roots, found a `..` segment
```

A file that is not valid JSON, or that names a variant or a field the schema does
not have, is located by line and column instead of by field path, since there is
no descriptor to point into yet:

```
Invalid platform descriptor acme.json: field `line 14 column 27` could not be
  read: unknown variant `file`, expected one of `registry`, `synthetic`,
  `logTail`, `nativeHook`
```

There is no silent failure at run time: a field the engine does not understand is
a rejection, not something discovered halfway through a switch. Unknown fields
are refused too, so a typo in a name reads as an error rather than as a setting
that quietly does nothing.

Refusals are not swallowed. The settings screen lists every file the folder holds
that did not load, with the message above, next to the ones that did.

## Trying one without installing it

Two ways to see what a descriptor would do before it does it.

**In the app.** "Add from a file" reads the file, validates it and shows the
sandbox roots and every file, registry value and process a switch would read,
copy, write or close. Nothing is written until the file is actually added, and a
descriptor whose id is already shipped says so instead of offering a button.

**From the CLI**, for a platform that is already installed:

```
accshift dry-run <platform> <account-id>
```

Both print the same plan, built by the same code that performs the switch. The
plan never claims to have run.

`accshift descriptors` is the other half: it lists what the folder loaded and
every file it refused, with the field that caused it. That is the fastest loop
while writing one.

## Adding a platform

1. Copy the closest shipped descriptor from
   `crates/accshift-core/src/platforms/descriptor/descriptors/`. `jagex.json` is
   the smallest complete one.
2. Put it in the descriptor folder the settings screen shows, or add it from a
   file, and read the plan.
3. Iterate: edit, reload, read the refusal, fix the named field.
4. To propose it for the app itself, open a Platform request issue as described
   in [CONTRIBUTING](../.github/CONTRIBUTING.md) and attach the descriptor. A
   platform is hard to review without someone who owns an account on it.
