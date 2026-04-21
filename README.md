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
  <a href="#current-status"><img src="https://img.shields.io/badge/platform-Windows-0078D6?logo=windows" alt="Platform" /></a>
  <a href="https://tauri.app/"><img src="https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri" alt="Tauri" /></a>
  <a href="https://svelte.dev/"><img src="https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte" alt="Svelte" /></a>
</p>

## Current Status

| Platform             | Windows     | macOS           | Linux           |
| -------------------- | ----------- | --------------- | --------------- |
| Steam                | ✅ Done     | 🧪 CLI ready    | 🧪 CLI ready    |
| Riot Games           | ✅ Done     | 🚧 Possible     | ⛔ Not feasible |
| Battle.net           | ✅ Done     | 🚧 Possible     | ⛔ Not feasible |
| Epic Games           | ✅ Done     | 🚧 Possible     | 🚧 Possible     |
| Ubisoft Connect      | ✅ Done     | 🚧 Possible     | 🚧 Possible     |
| Roblox               | ✅ Done     | 🚧 Possible     | 🚧 Possible     |
| EA app               | 🚧 Possible | 🚧 Possible     | ⛔ Not feasible |
| Discord              | 🚧 Possible | 🚧 Possible     | 🚧 Possible     |
| Rockstar Launcher    | 🚧 Possible | ⛔ Not feasible | 🚧 Possible     |
| GeForce Now          | 🚧 Possible | 🚧 Possible     | 🚧 Possible     |
| HoYoverse / HoYoPlay | 🚧 Possible | ⛔ Not feasible | ⛔ Not feasible |
| Minecraft Launcher   | 🚧 Possible | 🚧 Possible     | 🚧 Possible     |

- `✅ Done` — GUI and CLI implemented and working
- `🧪 CLI ready` — non-Windows support landed, awaiting on-target verification
- `🚧 Possible` — feasible, priority goes to user requests
- `⛔ Not feasible` — not realistic for this OS

Users can propose new platforms through [GitHub Issues](https://github.com/klNuno/accshift/issues).

## Installation

Download the latest installer from [Releases](https://github.com/klNuno/accshift/releases).

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
automation. It reads and writes the same config as the GUI — running
both at once is safe thanks to an exclusive lock on mutating operations.

### Install

The CLI binary is produced alongside the GUI by `pnpm tauri build`
(output: `target/release/accshift.exe` on Windows, `target/release/accshift`
on Linux/macOS). Drop it anywhere on your `PATH`, or run it by its
absolute path.

### Commands

```bash
accshift platforms               # list platforms known to this build
accshift list <platform>         # list accounts for a platform
accshift switch <platform> <account-id>
    [--steam-mode online|invisible]
    [--shutdown graceful|force]
    [--run-as-admin]
    [--launch-options "..."]
```

Example:

```
$ accshift list steam
  ACCOUNT        PERSONA                         STEAM ID
* microtel91     meetsu (low cortisol edition)   76561198008071583
  kuba3136       hom dafair                      76561198155223381
  dzirt522       chien congelé                   76561198120679570
  ...

84 accounts.  * = currently signed in
```

Output format:

- **Default**: a readable table for humans on a TTY, auto-switched to
  JSON when stdout is piped (so scripts and AI tools get a stable
  contract without extra flags).
- `--json` forces the JSON envelope everywhere.
- `--human` forces the table even when piped (useful for `| less`).
- Errors always go to stderr so stdout stays parseable.

### Output schema

```json
{ "schema": "accshift.v1", "ok": true, "command": "list",
  "data": { "platform": "steam", "accounts": [ ... ] } }

{ "schema": "accshift.v1", "ok": false, "command": "switch",
  "error": { "code": "lock_contended", "message": "..." } }
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

This project is not affiliated with Valve, Blizzard, Riot Games, Epic Games, Ubisoft, or Roblox Corporation. Use at your own risk.
