<h1 align="center">accshift</h1>
<p align="center">Fast multi-platform desktop account switcher, built with Tauri 2 and Svelte 5.</p>

<p align="center">
  <img src="./public/logo.svg" alt="accshift logo" width="140" />
</p>

<p align="center">
  <a href="https://github.com/klNuno/accshift/releases"><img src="https://img.shields.io/github/v/release/klNuno/accshift?display_name=tag" alt="Release" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/github/license/klNuno/accshift" alt="License" /></a>
  <a href="https://github.com/klNuno/accshift/stargazers"><img src="https://img.shields.io/github/stars/klNuno/accshift" alt="Stars" /></a>
  <a href="https://github.com/klNuno/accshift/issues"><img src="https://img.shields.io/github/issues/klNuno/accshift" alt="Issues" /></a>
  <a href="#current-status"><img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078D6" alt="Platform" /></a>
  <a href="https://tauri.app/"><img src="https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri" alt="Tauri" /></a>
  <a href="https://svelte.dev/"><img src="https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte" alt="Svelte" /></a>
</p>

> [!NOTE]
> See the [wiki](https://github.com/klNuno/accshift/wiki) for the full user guide.

## Current Status

| Platform             | Windows         | macOS           | Linux           |
| -------------------- | --------------- | --------------- | --------------- |
| Steam                | ✅ Done         | ✅ Done         | ✅ Done         |
| Riot Games           | ✅ Done         | 🚧 Possible     | ⛔ Not feasible |
| Battle.net           | ✅ Done         | ✅ Done         | ⛔ Not feasible |
| Epic Games           | ✅ Done         | 🚧 Possible     | 🚧 Possible     |
| Ubisoft Connect      | ✅ Done         | 🚧 Possible     | 🚧 Possible     |
| Roblox               | ✅ Done         | 🚧 Possible     | 🚧 Possible     |
| GOG Galaxy           | 🧪 Need testing | 🚧 Possible     | ⛔ Not feasible |
| Jagex Launcher       | 🧪 Need testing | 🚧 Possible     | ⛔ Not feasible |
| Discord              | 🧪 Need testing | 🚧 Possible     | 🚧 Possible     |
| EA app               | 🚧 Possible     | 🚧 Possible     | ⛔ Not feasible |
| Rockstar Launcher    | 🚧 Possible     | ⛔ Not feasible | 🚧 Possible     |
| GeForce Now          | 🚧 Possible     | 🚧 Possible     | 🚧 Possible     |
| HoYoverse / HoYoPlay | 🚧 Possible     | ⛔ Not feasible | ⛔ Not feasible |
| Minecraft Launcher   | 🚧 Possible     | 🚧 Possible     | 🚧 Possible     |

- `✅ Done`: GUI and CLI implemented and verified on target
- `🧪 Need testing`: implemented, may still have bugs
- `🚧 Possible`: feasible, priority goes to user requests
- `⛔ Not feasible`: not realistic for this OS

Users can propose new platforms through [GitHub Issues](https://github.com/klNuno/accshift/issues).

## Features

- **One-click account switching** for Steam, Riot Games, Battle.net, Epic Games, Ubisoft Connect, Roblox, GOG Galaxy, Jagex Launcher and Discord: no passwords stored; sensitive cookies, tokens and session snapshots are encrypted at rest.
- **Personas**: group one account per platform under a single identity and switch them all in one click.
- **Streamer mode**: automatically blurs account names and avatars when OBS, Streamlabs, XSplit or Twitch Studio is running.
- **Folders, search, command palette and keyboard navigation** to manage large account collections.
- **CLI and deep links** (`accshift://`) for scripting, Stream Deck and automation.
- **English and French UI**, light/dark/custom themes.

## Installation

Grab the build for your OS from [Releases](https://github.com/klNuno/accshift/releases):

- **Windows**: NSIS or MSI installer
- **Linux**: deb, rpm or AppImage
- **macOS**: dmg (unsigned for now, run `xattr -cr /Applications/Accshift.app` once if Gatekeeper complains)

## Building from source

```bash
pnpm install
pnpm tauri build
```

## Development

```bash
pnpm install
pnpm tauri dev
```

## CLI

`accshift` also ships as a command-line binary for scripting and AI
automation. It reads and writes the same config as the GUI: running
both at once is safe thanks to an exclusive lock on mutating operations.

### Install

- **Windows**: the desktop installer ships the CLI next to the app and adds
  the install directory to your user `PATH`; `accshift` works in any new
  terminal right after install. A standalone
  `accshift-cli_<version>_x64.exe` binary is also
  available on [Releases](https://github.com/klNuno/accshift/releases).
- **Linux**: the deb/rpm packages install the CLI to `/usr/bin` alongside the
  app. A standalone `accshift-cli_<version>_linux_x86_64` binary is also on
  Releases.
- **macOS**: the `.app` bundles the CLI inside `Contents/MacOS` (symlink it
  onto your `PATH`, e.g. into `/usr/local/bin`). A standalone
  `accshift-cli_<version>_macos_aarch64` binary is also on Releases.

Building from source (`pnpm tauri build`) produces the binary at
`target/release/accshift` (`accshift.exe` on Windows).

### Commands

```bash
accshift platforms               # list platforms known to this build
accshift list <platform>         # list accounts for a platform
accshift list <platform> --folder <name>
accshift switch <platform> <account-id>
    [--online | --invisible]
    [--graceful | --force]
    [--admin | --no-admin]
    [--launch-options "..."]
```

Example:

```
$ accshift list steam
  ACCOUNT      NAME                 STEAM ID
* alice        Alice                76561198000000001
  bob          Bob the Builder      76561198000000002
  carol        carol_gg             76561198000000003

3 accounts.  * = currently signed in
```

Output format:

- **Default**: a readable table for humans on a TTY, auto-switched to
  JSON when stdout is piped (so scripts and AI tools get a stable
  contract without extra flags).
- `--json` forces the JSON envelope everywhere.
- Errors always go to stderr so stdout stays parseable.

### Output schema

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

### Exit codes

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

## Project Structure

```text
src/lib/                          # Svelte frontend (GUI)
  app/                            # app lifecycle, dialogs, navigation
  features/folders notifications settings
  platforms/                      # per-platform UI adapters
  shared/components contextMenu platform ...
  storage/                        # client storage layer

crates/
  accshift-core/                  # platform logic, config, storage, OS
    src/
      platforms/steam riot ...    # platform implementations
      os/windows linux macos      # per-OS primitives (sysinfo/open/keyring)
      context.rs                  # AppContext trait (replaces tauri::AppHandle)
      lock.rs                     # fs4 exclusive lock
      runtime.rs                  # tokio block_on helper
      config storage logging themes
  accshift-cli/                   # CLI binary (list, switch, platforms)

src-tauri/                        # Tauri GUI thin wrapper
  src/main.rs commands.rs app_runtime.rs tauri_context.rs
```

## Disclaimer

This project is not affiliated with Valve, Blizzard, Riot Games, Epic Games, Ubisoft, Roblox Corporation, CD PROJEKT (GOG), Jagex, or Discord Inc. Use at your own risk.
