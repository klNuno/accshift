<h1 align="center">accshift</h1>
<p align="center">Switch Steam, Valorant, League of Legends, Battle.net, Epic Games, Ubisoft and Roblox accounts in one click. No passwords stored. Windows, macOS and Linux. Built with Tauri 2 and Svelte 5.</p>

<p align="center">
  <img src="./.github/assets/demo-switch.webp" alt="Switching Steam accounts from the grid, then from the Ctrl+K command palette" />
</p>

<p align="center">
  <a href="https://github.com/klNuno/accshift/releases"><img src="https://img.shields.io/github/v/release/klNuno/accshift?display_name=tag" alt="Release" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/github/license/klNuno/accshift" alt="License" /></a>
  <a href="https://github.com/klNuno/accshift/stargazers"><img src="https://img.shields.io/github/stars/klNuno/accshift" alt="Stars" /></a>
  <a href="https://github.com/klNuno/accshift/issues"><img src="https://img.shields.io/github/issues/klNuno/accshift" alt="Issues" /></a>
  <a href="#supported-platforms"><img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078D6" alt="Platform" /></a>
  <a href="https://tauri.app/"><img src="https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri" alt="Tauri" /></a>
  <a href="https://svelte.dev/"><img src="https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte" alt="Svelte" /></a>
</p>

> [!NOTE]
> See the [wiki](https://github.com/klNuno/accshift/wiki) for the full user guide.

## Supported platforms

Nine integrations ship today, each verified on the systems listed:

| Platform                                               | Verified on            |
| ------------------------------------------------------ | ---------------------- |
| Steam                                                  | Windows, macOS, Linux  |
| Riot Games (Valorant, League of Legends, TFT)          | Windows                |
| Battle.net (Overwatch 2, Diablo IV, WoW, Call of Duty) | Windows, macOS         |
| Epic Games (Fortnite, Rocket League)                   | Windows                |
| Ubisoft Connect (Rainbow Six Siege, The Division 2)    | Windows                |
| Roblox                                                 | Windows                |
| GOG Galaxy (Cyberpunk 2077, The Witcher 3)             | Windows, in testing    |
| Jagex Launcher (RuneScape, Old School RuneScape)       | Windows, in testing    |
| Discord                                                | Windows, in testing    |

Five more launchers (EA app, Rockstar, GeForce Now, HoYoPlay, Minecraft) are
feasible but not built yet, and a few combinations are not realistic on a given
OS at all. The full per-OS grid is in
[docs/platform-support.md](./docs/platform-support.md), and new platforms are
picked from what users ask for through
[GitHub Issues](https://github.com/klNuno/accshift/issues/new/choose).

## Features

- **One-click account switching** for Steam, Riot Games (Valorant, League of Legends), Battle.net (Overwatch 2, Diablo IV), Epic Games (Fortnite, Rocket League), Ubisoft Connect (Rainbow Six Siege), Roblox, GOG Galaxy, Jagex Launcher and Discord: no passwords stored; sensitive cookies, tokens and session snapshots are encrypted at rest.
- **Personas**: group one account per platform under a single identity and switch them all in one click.
- **Streamer mode**: automatically blurs account names and avatars when OBS, Streamlabs, XSplit, Wirecast or Twitch Studio is running.
- **Folders, search, command palette and keyboard navigation** to manage large account collections.
- **CLI and deep links** (`accshift://`) for scripting, Stream Deck and automation.
- **UI in 7 languages** (English, Spanish, French, Portuguese, Brazilian Portuguese, Russian, Simplified Chinese), light/dark/custom themes.

### Organise a large library

<p align="center">
  <img src="./.github/assets/demo-organize.webp" alt="Recoloring an account card from the right-click menu, then opening a folder of smurf accounts" />
</p>

### One app, every platform and theme

<p align="center">
  <img src="./.github/assets/demo-themes.webp" alt="Switching to the Riot Games tab, then changing the app theme from the settings panel" />
</p>

## Installation

Grab the build for your OS from [Releases](https://github.com/klNuno/accshift/releases):

- **Windows**: NSIS or MSI installer
- **Linux**: deb, rpm or AppImage
- **macOS**: dmg (unsigned for now, run `xattr -cr /Applications/Accshift.app` once if Gatekeeper complains)

## Privacy

Accshift stores no passwords, and sensitive cookies, tokens and session
snapshots are encrypted at rest on your machine with OS-backed protection:
DPAPI on Windows, Secret Service on Linux, Keychain on macOS. The threat model,
what the optional PIN lock does and does not cover, and how to report a
vulnerability are all in the [security policy](./.github/SECURITY.md).

Usage telemetry is a handful of anonymous counters. Nothing is sent before you
finish the first-launch screen, and **one switch in Settings, Privacy turns it
all off for good**. The app is identical either way, and no feature is gated on
it.

What is never sent, in any mode: account names, platform identifiers such as
SteamID, passwords, tokens, cookies, persona or folder names, file paths, and
your IP address. An event can say "an account was added on Steam"; it cannot say
which account. What is sent is nine counters, the app and OS version, the
locale, and a country code.

Everything is detailed in [docs/analytics.md](./docs/analytics.md): the full
event list with every field, a real example payload, where the data is stored,
and how to export or delete it. The client is under
[`crates/accshift-core/src/telemetry/`](./crates/accshift-core/src/telemetry)
and the server is in [`server/`](./server), both readable in a sitting.

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

Setup, coding standards and how to propose a new platform are in the
[contributing guide](./.github/CONTRIBUTING.md).

## CLI

`accshift` also ships as a command-line binary for scripting, Stream Deck
macros and AI automation. It reads and writes the same config as the GUI:
running both at once is safe thanks to an exclusive lock on mutating
operations.

```bash
accshift platforms               # list platforms known to this build
accshift list <platform>         # list accounts for a platform
accshift switch <platform> <account-id>
```

Output is a table on a TTY and JSON when piped, so scripts get a stable
contract without an extra flag. Install steps, every flag, the versioned JSON
envelope and the exit codes are in [docs/cli.md](./docs/cli.md).

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
