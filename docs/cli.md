# CLI reference

`accshift` ships as a command-line binary alongside the desktop app, for
scripting, Stream Deck macros and AI automation. It reads and writes the same
config as the GUI: running both at once is safe thanks to an exclusive lock on
mutating operations.

## Install

Windows
The desktop installer ships the CLI next to the app and adds the install
directory to your user `PATH`, so `accshift` works in any new terminal right
after install. A standalone `accshift-cli_<version>_x64.exe` binary is also
available on [Releases](https://github.com/klNuno/accshift/releases).

Linux
The deb and rpm packages install the CLI to `/usr/bin` alongside the app. A
standalone `accshift-cli_<version>_linux_x86_64` binary is also on Releases.

macOS
The `.app` bundles the CLI inside `Contents/MacOS`. Symlink it onto your `PATH`,
for example into `/usr/local/bin`. A standalone
`accshift-cli_<version>_macos_aarch64` binary is also on Releases.

Building from source (`cargo build --release -p accshift-cli`) produces the binary at
`<target-directory>/release/accshift`, or `accshift.exe` on Windows. The target
directory defaults to `target/`; Cargo configuration or `CARGO_TARGET_DIR` can
override it.

## Commands

```bash
accshift platforms               # list platforms known to this build
accshift list <platform>         # list accounts for a platform
accshift list <platform> --folder <name-or-id>
accshift folders <platform>      # folder tree with ids and account counts
accshift current [platform]      # account signed in on each platform
accshift search <query>          # accounts on every platform by name or note
accshift personas                # personas saved in the app
accshift persona <name-or-id>    # switch every account of a persona
accshift completions <bash|zsh|fish|powershell>
accshift switch <platform> <account-id>
    [--online | --invisible]
    [--graceful | --force]
    [--admin | --no-admin]
    [--launch-options "..."]
accshift dry-run <platform> <account-id>
accshift descriptors             # what the user descriptor folder holds
accshift diag check              # check local health invariants
accshift diag logs --since 30m    # inspect recent log records
accshift diag bundle             # write a local diagnostic report
```

Every one of these needs the "Allow the accshift CLI" toggle in the app
(Settings > General > Integrations). With it off they all exit 7 and do
nothing, `diag` included.

`--folder` takes a folder id (from `accshift folders`) or a name within the
selected platform, matched without case sensitivity, and includes accounts in
nested folders. An id wins over a name. A name two folders share is refused
with `ambiguous_folder` (exit 1), and the error lists each candidate's id and
path; the JSON error carries them under `error.details.candidates`. A
resolved folder's path is in the JSON `data.folderPath`.

```
$ accshift list steam --folder main
error: Folder name 'main' matches 2 folders on steam. Pass one of these ids with --folder:
  mbx41k2q9f  Main
  mbx5c7aa1z  Smurfs / main
  code: ambiguous_folder

$ accshift list steam --folder mbx5c7aa1z
Folder: Smurfs / main
...
```

### folders

`folders <platform>` prints the platform's folder tree, depth first, siblings
in the order the app shows them. `ACCOUNTS` counts the folder and everything
nested in it, as the folder store records them. JSON gives each folder its
`id`, `name`, `parentId`, `path`, `depth`, `accounts` (placed directly) and
`totalAccounts`.

```
$ accshift folders steam
ID          FOLDER    ACCOUNTS
mbx5c7aa1z  Smurfs    2
mbx6d0p3kk    main    1
mbx41k2q9f  Main      2

Folders: 3
```

### current

`current` asks each platform which account is signed in, with the same
detection the app uses for its "current" badge. `current <platform>` asks one.
A platform that cannot tell (client not installed, never signed in) answers
`unknown` rather than failing, so the command exits 0. JSON entries hold
`platform`, `status` (`known` or `unknown`), `accountId` (the id `switch`
takes) and `name`.

```
$ accshift current
PLATFORM    ACCOUNT  NAME
steam       alice    Alice
riot        unknown
battle-net  unknown
```

### search

`search <query>` looks through every platform's accounts for the text, without
case sensitivity, in the account id, its display name (persona name, label,
BattleTag), its secondary id (Riot ID, Steam ID, Roblox username) and its note
from the app. It exits 0 with no match. A platform whose accounts cannot be read is listed in
`skipped` (on stderr for people).

```
$ accshift search smurf
PLATFORM  ID     NAME   NOTE
steam     carol  Carol  Faceit smurf

Matches: 1
```

### personas and persona

`personas` lists the personas saved in the app with their assignments.
`persona <name-or-id>` switches every account of that persona, in the
persona's order, with the same platform switch, Steam settings, PIN prompt,
operation lock and exit codes as `switch`. A platform this build does not have
is skipped and reported, as the app does. The switch stops at the first
failure, and the report says which platforms switched, which one failed and
why, and which were not tried; the exit code is the failed step's (3 for an
account no longer listed, 1 for a platform error). A name two personas share
is refused with `ambiguous_persona` and their ids.

```
$ accshift persona Main
Switched to Main: steam (76561198000000001), riot (6b1f...).

$ accshift persona Main
error: Main stopped at riot (6b1f...): Riot profile not found
Switched before the failure: steam (76561198000000001)
Not tried: epic (e1)
  code: unknown_account
```

The JSON error carries the whole report under `error.details.report`:
`succeeded`, `failed` (`platformId`, `accountId`, `code`, `message`),
`notTried` and `skipped`.

### completions

`completions <shell>` prints a completion script generated from the same
definition the binary parses with, for `bash`, `zsh`, `fish` or `powershell`.
It always prints the raw script, piped or not, and like every subcommand it
needs the "Allow the accshift CLI" toggle.

```bash
accshift completions bash > ~/.local/share/bash-completion/completions/accshift
accshift completions zsh > "${fpath[1]}/_accshift"
accshift completions fish > ~/.config/fish/completions/accshift.fish
accshift completions powershell >> $PROFILE
```

### Language

The text `folders`, `current`, `search`, `personas`, `persona` and the
`--folder` errors print for people follows the app's language (Settings),
else the OS language: English, French, Spanish, Portuguese, Brazilian
Portuguese, Russian or Chinese. The older commands still print English. JSON output, error messages included, is always English, so a script
reads the same text on every machine; its contract is the `code`.

The full `diag` command reference is in [logging.md](./logging.md).

`--graceful` asks the launcher to close itself and waits for it, which is what
you want by default because a launcher killed mid-write can corrupt its own
config. `--force` terminates it instead, for the cases where it will not go.

`dry-run` prints the switch instead of performing it: every file, folder and
registry value it would read, copy back or delete, every process it would
close, and the launcher it would start. It walks the same descriptor the real
switch walks, so the two cannot disagree. It opens nothing for writing and
takes no lock, so it is safe to run at any time, including while the GUI is
busy.

Platforms still implemented in code (Steam, Battle.net, Riot, Roblox) have no
plan to show and answer `dry_run_unsupported`.

`descriptors` reads the folder where a user drops platforms of their own and
reports both halves: the descriptors that loaded, and every file that was
refused with the field that caused it. A platform missing from `platforms` is
explained here rather than silently absent. It exits zero either way, rejected
files included: the command was asked what the folder holds and it answered, so
a script reads `rejected` instead of guessing from a status that would also mean
"could not look". The format itself is in
[platform-descriptors.md](./platform-descriptors.md).

Example:

```
$ accshift list steam
  ACCOUNT      NAME                 STEAM ID
* alice        Alice                76561198000000001
  bob          Bob the Builder      76561198000000002
  carol        carol_gg             76561198000000003

3 accounts.  * = currently signed in
```

```
$ accshift dry-run gog 51000000000000000
Dry run: switch gog to 51000000000000000. Nothing below is written.

Roots (a step outside these is refused):
  C:\Users\you\AppData\Local\GOG.com
  C:\ProgramData\GOG.com

  capture  C:\Users\you\AppData\Local\GOG.com\Galaxy\Configuration\config.json  <- ...\snapshots\51000000000000001\config.json
  capture  HKCU\Software\GOG.com\Galaxy\refreshToken  <- ...\snapshots\51000000000000001\registry_refresh_token.txt
  close    GalaxyClient.exe
  restore  C:\Users\you\AppData\Local\GOG.com\Galaxy\Configuration\config.json  <- ...\snapshots\51000000000000000\config.json
  launch   C:\Program Files (x86)\GOG Galaxy\GalaxyClient.exe
```

## Output format

The default output is a readable table for humans on a TTY, and switches to
JSON automatically when stdout is piped, so scripts and AI tools get a stable
contract without passing an extra flag.

- `--json` forces the JSON envelope everywhere, TTY included.
- Errors always go to stderr, so stdout stays parseable even on failure.

The `schema` field is versioned. Consumers should check it rather than assume:
a future incompatible shape ships as `accshift.v2`, and `accshift.v1` keeps
meaning what it means here.

### Success envelope

```json
{
  "schema": "accshift.v1",
  "ok": true,
  "command": "list",
  "data": {
    "platform": "steam",
    "folder": null,
    "accounts": [],
    "current": null
  }
}
```

### Error envelope

```json
{
  "schema": "accshift.v1",
  "ok": false,
  "command": "switch",
  "error": {
    "code": "lock_contended",
    "message": "Another accshift instance is running. Retry once it finishes, or close the GUI."
  }
}
```

## Exit codes

| Code | Meaning                                  |
| ---- | ---------------------------------------- |
| 0    | Success                                  |
| 1    | Generic error                            |
| 2    | Unknown platform on this OS              |
| 3    | Unknown account                          |
| 4    | Another accshift instance holds the lock |
| 5    | I/O error (paths, permissions)           |
| 6    | PIN missing, unavailable, or incorrect   |
| 7    | CLI disabled in Settings                 |

Code 4 is retryable: the GUI and the CLI share one config, so a mutating
operation takes an exclusive lock and a second one waits rather than corrupting
it. Retry once the other instance finishes.

Codes 6 and 7 are deliberate refusals, not failures. Every subcommand respects
the "Allow the accshift CLI" setting. Switching also checks the PIN lock
configured in the app. An automated pipeline that
starts returning 7 has been disabled through that setting. What the PIN
lock does and does not protect is covered in the
[security policy](../.github/SECURITY.md).

Code 7 covers every subcommand, `platforms`, `descriptors` and all of `diag`
included, and the check runs before the command is handed its arguments, so a
refused run reads nothing and writes nothing. There is no exemption list: the
`CLI_GATE_EXEMPT` const in `crates/accshift-cli/src/main.rs` is empty on
purpose. Until 1.0.4 the toggle only covered `list`, `switch` and `dry-run`, so
`diag bundle` still wrote a report carrying the redacted config summary on a
machine whose owner had switched the CLI off. Diagnostics stay reachable
without the CLI: the app has its own diagnostics screen.

A settings file that exists but cannot be read, with no usable `.bak` beside
it, leaves the toggle unknown. Every subcommand then exits with 5 rather than
assuming the CLI is allowed.
