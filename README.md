<p align="center">
  <img src="public/logo.svg" width="128" height="128" alt="zazaSwitcher">
</p>

<h1 align="center">zazaSwitcher</h1>

<p align="center">
  <strong>Fast Steam account switcher for Windows</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows-0078D6?style=flat-square&logo=windows" alt="Platform">
  <img src="https://img.shields.io/badge/Tauri-2.0-FFC131?style=flat-square&logo=tauri" alt="Tauri">
  <img src="https://img.shields.io/badge/Svelte-5-FF3E00?style=flat-square&logo=svelte" alt="Svelte">
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License">
</p>

---

## Installation

Download the latest release from the [Releases](https://github.com/yourusername/zazaSwitcher/releases) page.

Or build from source:

```bash
pnpm install
pnpm tauri build
```

## Features

- ✨ **Fast Account Switching** - Switch between Steam accounts in seconds
- 📁 **Folder Organization** - Organize your accounts with custom folders
- 🎨 **Modern UI** - Clean interface with Svelte 5 & Tauri 2
- 🔔 **Smart Notifications** - Get notified about profile picture updates
- 🎯 **Launch Modes** - Start Steam in online or invisible mode
- 🔄 **Platform Ready** - Extensible architecture for future platforms (Riot, Epic, etc.)

## Development

```bash
pnpm install
pnpm tauri dev
```

### Project Structure

The codebase follows a feature-based architecture:

```
src/lib/
├── features/          # Platform & feature modules
│   ├── steam/        # Steam integration
│   ├── folders/      # Folder management
│   ├── settings/     # App settings
│   └── notifications/# Notification system
└── shared/           # Shared utilities
    ├── components/   # Generic components
    ├── platform.ts   # Platform abstraction
    └── dragAndDrop.svelte.ts
```

See [RESTRUCTURING.md](RESTRUCTURING.md) for detailed architecture documentation.

## License

[MIT](LICENSE)


#TODO

detecter les comm ban et vac/game ban
faire logique riot
faire un affichage en liste, avec le logo et les autres infos du compte qui apparaissent à droite comme tah album photo explorateur win
