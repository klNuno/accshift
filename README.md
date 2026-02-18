# Accshift
Fast desktop account switcher for Steam on Windows, built with Tauri 2 and Svelte 5.

[![Release](https://img.shields.io/github/v/release/klNuno/zazaSwitcher?display_name=tag)](https://github.com/klNuno/zazaSwitcher/releases)
[![License](https://img.shields.io/github/license/klNuno/zazaSwitcher)](./LICENSE)
[![Stars](https://img.shields.io/github/stars/klNuno/zazaSwitcher)](https://github.com/klNuno/zazaSwitcher/stargazers)
[![Issues](https://img.shields.io/github/issues/klNuno/zazaSwitcher)](https://github.com/klNuno/zazaSwitcher/issues)
[![Platform](https://img.shields.io/badge/platform-Windows-0078D6?logo=windows)](#requirements)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri)](https://tauri.app/)
[![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte)](https://svelte.dev/)

## What It Does
- Switches Steam accounts quickly from a single UI.
- Organizes accounts with folders and drag-and-drop.
- Supports profile/avatar refresh and ban checks (Steam Web API).
- Includes configurable startup mode, launch options, and optional PIN lock.

## Requirements
- Windows 10/11
- Node.js 20+
- pnpm 9+
- Rust stable toolchain
- Visual Studio Build Tools (for Tauri builds on Windows)

## Installation
### From Releases
Download the latest installer from:
`https://github.com/klNuno/zazaSwitcher/releases`

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

## Roadmap
- Add more platform adapters (Riot and others).
- Improve account metadata sync and diagnostics.
- Add CI workflows and automated tests.

## Contributing
Issues and pull requests are welcome.

Recommended flow:
1. Open an issue to discuss the change.
2. Create a branch from `main`.
3. Submit a pull request with context, screenshots, and test notes.

## License
MIT. See `LICENSE`.

## Disclaimer
This project is not affiliated with Valve or Steam.
