# Contributing to accshift

Thanks for taking the time to contribute. This document covers the setup, the
checks CI enforces, and what is expected of a pull request.

## Prerequisites

- A stable Rust toolchain with the `rustfmt` and `clippy` components.
- Node.js 22 and pnpm 10.32.1, only to build installers (the Tauri bundler) and
  to work on the telemetry worker. pnpm is pinned by `packageManager` in
  `package.json`, so `corepack enable` gives you the right version.

On Ubuntu, the packages CI installs are:

```bash
sudo apt-get install -y libdbus-1-dev libgtk-3-dev libxkbcommon-dev pkg-config
```

## Getting started

```bash
cargo run -p accshift-egui
```

`node crates/accshift-egui/bundle/bundle.mjs --build` builds the installers for
your platform after `pnpm install`.

## Checks

CI runs the `rust` job on Windows, Ubuntu and macOS. Every step must pass on
all three. Run the equivalents locally before opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`node scripts/gates.mjs` runs those three and the worker typecheck and tests,
then prints each exit code; `node scripts/gates.mjs clippy rust` runs only the
named gates. It needs `server/node_modules` installed.

The telemetry worker in `server/` is a separate pnpm package:

```bash
pnpm --dir server install --frozen-lockfile
pnpm --dir server exec tsc --noEmit
pnpm --dir server exec vitest run
```

`cargo audit --file Cargo.lock` runs on Ubuntu only. If your change touches
dependencies, run it locally too.

## Project layout

See the "Project Structure" section of the [README](../README.md#project-structure).

The Cargo workspace has five members: `crates/accshift-core` (platform logic,
config, storage, OS primitives), `crates/accshift-cli` (the CLI binary),
`crates/accshift-app` (the UI-free app logic), `crates/accshift-theme` (the
theme engine) and `crates/accshift-egui` (the GUI).

The CLI and GUI both sit on top of `crates/accshift-core`, so a platform change
usually touches both surfaces. When you add or modify a platform, check that the
CLI (`accshift platforms`, `list`, `switch`) and the GUI behave consistently, and
that the exclusive lock is still respected on mutating operations.

## Proposing a new platform

Most platforms are a JSON descriptor rather than code, so you can build and try
one without compiling the app: see
[platform-descriptors.md](../docs/platform-descriptors.md) for the format, the
sandbox rules and the dry run that shows what a switch would touch.

Open a GitHub Issue using the **Platform request** template rather than sending
an unsolicited implementation, and attach your descriptor if you have one. The
template asks for the launcher name, the operating systems it should support,
how the launcher stores its session, and whether you can help test. Platform
work is hard to review without someone who owns an account on that service, so
the testing answer matters.

The README's [Supported platforms](../README.md#supported-platforms) table
lists OS support and marks integrations with limited user feedback.

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
