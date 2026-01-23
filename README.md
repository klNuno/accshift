<p align="center">
  <img src="public/logo.svg" width="128" height="128" alt="zazaSwitcher">
</p>

<h1 align="center">zazaSwitcher</h1>

<p align="center">
  <strong>Fast Steam account switcher for Windows</strong>
</p>

<p align="center">
  <a href="#features">Features</a> •
  <a href="#installation">Installation</a> •
  <a href="#development">Development</a> •
  <a href="#license">License</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows-0078D6?style=flat-square&logo=windows" alt="Platform">
  <img src="https://img.shields.io/badge/Tauri-2.0-FFC131?style=flat-square&logo=tauri" alt="Tauri">
  <img src="https://img.shields.io/badge/Svelte-5-FF3E00?style=flat-square&logo=svelte" alt="Svelte">
  <img src="https://img.shields.io/badge/Rust-1.70+-DEA584?style=flat-square&logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License">
</p>

---

## Features

- 🚀 **Instant switching** — One click to switch accounts
- 🖼️ **Profile pictures** — Fetches avatars from Steam
- 🎨 **Minimal UI** — Clean, dark interface
- ⚡ **Lightweight** — Built with Tauri, under 5MB

## Installation

Download the latest release from the [Releases](https://github.com/yourusername/zazaSwitcher/releases) page.

Or build from source:

```bash
pnpm install
pnpm tauri build
```

## Development

```bash
# Install dependencies
pnpm install

# Run in development mode
pnpm tauri dev
```

## How it works

1. Reads saved accounts from Steam's `loginusers.vdf`
2. Updates the `AutoLoginUser` registry key
3. Restarts Steam with the selected account

## Tech Stack

| Layer | Technology |
|-------|------------|
| Frontend | Svelte 5, TypeScript, Tailwind CSS |
| Backend | Rust, Tauri 2.0 |
| Build | Vite, pnpm |

## License

MIT
