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
<table>
  <tr>
    <td valign="top" width="50%">

<p><strong>Platform Support</strong></p>
<table>
  <thead>
    <tr>
      <th>Platform</th>
      <th>Status</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Steam</td>
      <td>✅ Supported</td>
    </tr>
    <tr>
      <td>Riot Games</td>
      <td>❌ Not supported yet</td>
    </tr>
  </tbody>
</table>

  </td>
    <td valign="top" width="50%">

<p><strong>OS Support</strong></p>
<table>
  <thead>
    <tr>
      <th>OS</th>
      <th>Status</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Windows</td>
      <td>✅ Supported</td>
    </tr>
    <tr>
      <td>Linux</td>
      <td>❌ Not tested</td>
    </tr>
    <tr>
      <td>macOS</td>
      <td>❌ Not tested</td>
    </tr>
  </tbody>
</table>

  </td>
  </tr>
</table>

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
