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
| Steam                | ✅ Done     | 🚧 Possible     | 🚧 Possible     |
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

- `✅ Done` — implemented and working
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

## Project Structure

```text
src/lib/
  app/                          # app lifecycle, dialogs, navigation
  features/
    folders/                    # folder organization
    notifications/              # toast system
    settings/                   # settings store & UI
  platforms/
    battle-net/                 # Battle.net adapter, API, context menu
    epic/                       # Epic Games adapter, API, context menu
    riot/                       # Riot Games adapter, API, context menu
    roblox/                     # Roblox adapter, API, context menu
    steam/                      # Steam adapter, API, context menu, bulk edit
    ubisoft/                    # Ubisoft Connect adapter, API, context menu
    platformApi.ts              # shared platform API factory
    registry.ts                 # platform registry
  shared/
    components/                 # AccountCard, ListView, dialogs, etc.
    contextMenu/                # context menu builders
    platform.ts                 # platform types & interfaces
    useAccountLoader.svelte.ts  # account state management
  storage/                      # client storage layer

src-tauri/src/
  commands.rs                   # Tauri command handlers
  config.rs                     # app config (portable + local split)
  storage.rs                    # file storage, migrations, manifests
  platforms/
    battle_net.rs               # Battle.net switching & setup
    epic.rs                     # Epic Games switching & setup
    riot.rs                     # Riot session capture & switching
    roblox.rs                   # Roblox auth ticket switching
    ubisoft.rs                  # Ubisoft Connect switching & setup
    steam/
      mod.rs                    # Steam service & setup
      accounts.rs               # Steam account switching
      bans.rs                   # Steam ban checking
      bulk_edit.rs              # Steam bulk edit operations
      profile.rs                # Steam profile info
      vdf.rs                    # VDF parser
  os/                           # OS-specific APIs (Windows, DPAPI, process mgmt)
```

## Disclaimer

This project is not affiliated with Valve, Blizzard, Riot Games, Epic Games, Ubisoft, or Roblox Corporation. Use at your own risk.
