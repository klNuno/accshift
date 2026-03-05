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


## Current Status
| Platform | Windows | Linux | macOS |
| --- | --- | --- | --- |
| Steam | ✅ Supported | ⛔ Not supported yet | ⛔ Not supported yet |
| Riot Games | ✅ Supported | ⛔ Not supported yet | ⛔ Not supported yet |
| Battle.net | 🚧 Planned | 🚧 Planned | 🚧 Planned |
| Epic Games | 🚧 Planned | 🚧 Planned | 🚧 Planned |

## Highlights
- Windows support for Steam and Riot Games account management.
- Async add-account setup flow for Steam and Riot with pending card + polling.
- OS-aware platform availability in the UI (unsupported/planned platforms are disabled).
- Optional Riot "last login" display (manual, disabled by default).
- Riot setup/switch now avoids killing active game processes (LoL/Valorant must be closed first).
- Steam avatar refresh keeps the previous avatar when remote data is empty/invalid.

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
- Riot snapshot data is currently not encrypted at rest.
- Riot setup/switch only terminates Riot client processes, not game binaries.
- Steam API keys are encrypted at rest using OS-level secret storage.

## Project Structure
```text
src/lib/
  features/
    folders/
    notifications/
    settings/
  platforms/
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
    riot.rs
    steam.rs
  steam/
    accounts.rs
    bans.rs
    profile.rs
```

## License
MIT. See [LICENSE](./LICENSE).

## Disclaimer
This project is not affiliated with Valve or Riot Games.
