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

Building from source (`pnpm tauri build`) produces the binary at
`target/release/accshift`, or `accshift.exe` on Windows.

## Commands

```bash
accshift platforms               # list platforms known to this build
accshift list <platform>         # list accounts for a platform
accshift list <platform> --folder <name>
accshift switch <platform> <account-id>
    [--online | --invisible]
    [--graceful | --force]
    [--admin | --no-admin]
    [--launch-options "..."]
accshift dry-run <platform> <account-id>
accshift descriptors             # what the user descriptor folder holds
```

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

Codes 6 and 7 are deliberate refusals, not failures. The CLI can switch
accounts and reach session material, so it honours the PIN lock set in the app
and can be turned off entirely from Settings. An automated pipeline that starts
returning 7 has not broken, it has been switched off on purpose. What the PIN
lock does and does not protect is covered in the
[security policy](../.github/SECURITY.md).
