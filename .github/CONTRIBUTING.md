# Contributing to accshift

Thanks for taking the time to contribute. This document covers the setup, the
checks CI enforces, and what is expected of a pull request.

## Prerequisites

- Node.js 22 (the version CI uses).
- pnpm 10.32.1. It is pinned by `packageManager` in `package.json`, so
  `corepack enable` gives you the right version automatically.
- A stable Rust toolchain with the `rustfmt` and `clippy` components.
- Tauri 2 system dependencies for your OS. Follow
  [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/).

On Ubuntu, the packages CI installs are:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev libssl-dev pkg-config
```

## Getting started

```bash
pnpm install
pnpm tauri dev
```

`pnpm tauri build` produces a release bundle for your platform.

## Checks

CI runs the `validate` job on Windows, Ubuntu and macOS. Every step must pass on
all three. Run the equivalents locally before opening a pull request:

```bash
pnpm exec vp check              # Vite+ check
pnpm run check:frontend         # svelte-check
pnpm exec vp test               # frontend tests
pnpm exec vp build              # frontend build
pnpm run fmt:check              # cargo fmt --all --check
pnpm run clippy                 # cargo clippy --workspace -- -D warnings
pnpm run check:rust             # cargo check --workspace
pnpm run test:rust              # cargo test --workspace
```

`pnpm run check` chains the Vite+ check, `check:frontend` and `check:rust` in one
command.

Two extra steps CI performs that are easy to miss locally:

- The Rust checks need the staged CLI sidecar, which is gitignored. A fresh
  checkout must produce it first:
  ```bash
  cargo build --release -p accshift-cli
  node scripts/stage-cli.mjs
  ```
- The telemetry worker in `server/` is a separate pnpm package and is
  typechecked on its own:
  ```bash
  pnpm --dir server install --frozen-lockfile
  pnpm --dir server exec tsc --noEmit
  ```

`cargo audit --file Cargo.lock` runs on Ubuntu only. If your change touches
dependencies, run it locally too.

## Project layout

See the "Project Structure" section of the [README](../README.md#project-structure).

The Cargo workspace has three members: `src-tauri` (the Tauri GUI wrapper),
`crates/accshift-core` (platform logic, config, storage, OS primitives) and
`crates/accshift-cli` (the CLI binary).

The CLI and GUI both sit on top of `crates/accshift-core`, so a platform change
usually touches both surfaces. When you add or modify a platform, check that the
CLI (`accshift platforms`, `list`, `switch`) and the GUI behave consistently, and
that the exclusive lock is still respected on mutating operations.

## Proposing a new platform

Open a GitHub Issue using the **Platform request** template rather than sending
an unsolicited implementation. The template asks for the launcher name, the
operating systems it should support, how the launcher stores its session, and
whether you can help test. Platform work is hard to review without someone who
owns an account on that service, so the testing answer matters.

The README's "Current Status" table lists which platforms are already done,
implemented but untested, feasible, or not realistic for a given OS.

## Commits and pull requests

- Use conventional commit prefixes: `feat`, `fix`, `docs`, `chore`, `refactor`,
  `test`.
- Keep pull requests scoped to one change. Split unrelated refactors out.
- CI must be green before review. A red pipeline on any of the three operating
  systems blocks the merge.
- Describe which operating systems you tested on. The pull request template has
  checkboxes for this.
- Never include secrets, session tokens, cookies or captured account data in a
  diff, a log excerpt or a screenshot.

## Security issues

Do not open a public issue for a vulnerability. See [SECURITY.md](SECURITY.md).
