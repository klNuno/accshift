<h1 align="center">accshift</h1>
<p align="center">Fast multi-platform desktop account switcher, built with Tauri 2 and Svelte 5.</p>

<p align="center">
  <img src="./public/vite.svg" alt="accshift logo" width="140" />
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


## Current Status (very very early state)
| Platform | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Steam | ✅ Done | 🚧 Possible | 🚧 Possible |
| Riot Games | ✅ Done | 🚧 Possible | ⛔ Not feasible |
| Battle.net | ✅ Done | 🚧 Possible | ⛔ Not feasible |
| Epic Games | 🚧 Possible | 🚧 Possible | 🚧 Possible |
| Ubisoft Connect | 🚧 Possible | 🚧 Possible | 🚧 Possible |
| Roblox | 🚧 Possible | 🚧 Possible | ⛔ Not feasible |
| EA app | 🚧 Possible | 🚧 Possible | ⛔ Not feasible |
| Discord | 🚧 Possible | 🚧 Possible | 🚧 Possible |
| Rockstar Launcher | 🚧 Possible | ⛔ Not feasible | 🚧 Possible |
| GeForce Now | 🚧 Possible | 🚧 Possible | 🚧 Possible |
| HoYoverse / HoYoPlay | 🚧 Possible | ⛔ Not feasible | ⛔ Not feasible |
| Minecraft Launcher | 🚧 Possible | 🚧 Possible | 🚧 Possible |

Status:
- `✅ Done`: already implemented
- `🚧 Possible`: feasible and planned (priority to requested platforms from users)
- `⛔ Not feasible`: not realistic/supported for this OS

Note: some `🚧 Possible` entries on macOS/Linux may rely on compatibility layers (Wine/Proton/Heroic).

Users can propose new platforms through GitHub Issues.

## Installation
### From Releases
Download the latest installer from:
`https://github.com/klNuno/accshift/releases`

### From Source
```bash
pnpm install
pnpm tauri build
```

## Development
```bash
pnpm install
pnpm tauri dev
```

## Steam Ban Check Notes
- Ban checks use Steam `GetPlayerBans` API.
- A Steam Web API key is required (configure it in Settings).
- Requests are chunked and cached to reduce unnecessary API calls.
- Manual refresh can force a full refresh when needed.

## Riot Data & Security
- Riot profile metadata and session snapshots are stored locally in the app data directory.
- Snapshot files are used only for local restore/switch flows and are not uploaded by accshift.
- Riot account switching relies on a locally saved Riot session on this PC.
- Riot snapshot data is currently not encrypted at rest.
- Riot setup/switch only terminates Riot client processes, not game binaries.
- Steam API keys are encrypted at rest using OS-level secret storage.

## Recent Security Hardening
- Add-account setup identifiers are random UUIDs instead of timestamp-based IDs.
- Pending Steam, Riot, and Battle.net setup flows expire automatically after a short inactivity window.
- The local PIN lock now adds a short delay after a failed attempt to reduce trivial retry spam.
- The main desktop webview blocks external navigation in production.
- The desktop CSP no longer enables `unsafe-eval`.

## Battle.net Notes
- Battle.net account discovery is based on the local launcher configuration plus accshift local metadata.
- Display names use the native BattleTag when it becomes available locally.
- The add-account flow currently works by restarting the launcher and forcing a fresh login selection flow.
- Battle.net switching is designed for local desktop use on the same Windows machine.

## Installation Path Overrides
- Steam folder, Riot client executable, and Battle.net executable can all be overridden in Settings.
- If auto-detection fails, configure the launcher path manually from the platform settings tab.

## Project Structure
```text
src/lib/
  features/
    folders/
    notifications/
    settings/
  platforms/
    battle-net/
      adapter.ts
      battleNetApi.ts
      types.ts
    riot/
      adapter.ts
      riotApi.ts
      types.ts
    steam/
      adapter.ts
      steamApi.ts
      profileCache.ts
      types.ts
    registry.ts
  shared/
    components/
    platform.ts
    useAccountLoader.svelte.ts

src-tauri/src/
  commands.rs
  platforms/
    battle_net.rs
    riot.rs
    steam.rs
  steam/
    accounts.rs
    bans.rs
    profile.rs
```

## Disclaimer
This project is not affiliated with Valve, Blizzard, or Riot Games. Use it at your own risk.
