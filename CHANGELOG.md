# Changelog

## 0.2.8 - 2026-02-20

### Changes
- Add Steam account context menu action `Forget`.
- Add in-app confirmation modal for `Forget` (no external confirm popup).
- Keep custom account card color when the account is active.
- Refactor account name marquee behavior for cleaner logic and smoother looping.
- Use theme-aware AFK wave text color/glow via `--afk-text`.

### Fixes
- Restrict `Forget` to only remove the account from `config/loginusers.vdf`.
- Start marquee immediately on hover.
- Add a 2-second pause at both ends of the marquee cycle.
- Prevent abrupt marquee reset/jump between directions.
