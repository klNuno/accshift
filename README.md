# accshift
Fast multi-platform desktop account switcher, built with Tauri 2 and Svelte 5.

<p align="center">
  <img src="./public/vite.svg" alt="accshift logo" width="140" />
</p>

[![Release](https://img.shields.io/github/v/release/klNuno/accshift?display_name=tag)](https://github.com/klNuno/accshift/releases)
[![License](https://img.shields.io/github/license/klNuno/accshift)](./LICENSE)
[![Stars](https://img.shields.io/github/stars/klNuno/accshift)](https://github.com/klNuno/accshift/stargazers)
[![Issues](https://img.shields.io/github/issues/klNuno/accshift)](https://github.com/klNuno/accshift/issues)
[![Platform](https://img.shields.io/badge/platform-Windows-0078D6?logo=windows)](#requirements)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri)](https://tauri.app/)
[![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte)](https://svelte.dev/)

## Links
- Releases: [github.com/klNuno/accshift/releases](https://github.com/klNuno/accshift/releases)
- Issues: [github.com/klNuno/accshift/issues](https://github.com/klNuno/accshift/issues)
- License: [LICENSE](./LICENSE)

## What It Does
- Switches Steam accounts quickly from a single UI.
- Organizes accounts with folders and drag-and-drop.
- Supports profile/avatar refresh and ban checks (Steam Web API).
- Includes configurable startup mode, launch options, and optional PIN lock.

## Current Status
### OS Support
| OS | Status |
| --- | --- |
| Windows | ✅ Supported |
| Linux | ❌ Not tested |
| macOS | ❌ Not tested |

### Platform Support
| Platform | Status |
| --- | --- |
| Steam | ✅ Supported |
| Riot Games | ❌ Not supported yet |

## Requirements
- Windows 10/11
- Node.js 20+
- pnpm 9+
- Rust stable toolchain
- Visual Studio Build Tools (for Tauri builds on Windows)

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

## Project Structure
```text
src/lib/
  features/
    folders/
    notifications/
    settings/
  platforms/
    steam/
      adapter.ts
      steamApi.ts
      profileCache.ts
      types.ts
  shared/
    components/
    platform.ts
    useAccountLoader.svelte.ts

src-tauri/src/
  commands.rs
  steam/
    accounts.rs
    bans.rs
    profile.rs
```

## License
MIT. See [LICENSE](./LICENSE).

## Disclaimer
This project is not affiliated with Valve or Riot Games.
