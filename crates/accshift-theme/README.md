# accshift-theme

The accshift theme engine, ported from the former TypeScript one (`src/lib/theme/`). It answers with concrete values (RGBA colours, lengths, numbers) that match what the WebView computed for the same theme. The default tokens are `src/root.css`, the built-in theme documents `themes/*.json`. No egui dependency; `serde` and `serde_json` only.

Standalone like `crates/accshift-egui`: it has its own empty `[workspace]`, and the root `Cargo.toml` excludes it.

## What it ports

| TypeScript                                                                                                                                                             | Rust                     |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| `tokens.ts`: token specs, density scale, choices, value validation                                                                                                     | `tokens`                 |
| `schema.ts`: parse, validate, serialize, duplicate, `resolveThemeTokens`                                                                                               | `schema`                 |
| `themes.ts`: built-in documents, `extends`, custom theme payloads, `resolveThemeSurfaceOpacities`, `themeUsesLiquidGlass`, the variables `applyThemeToDocument` writes | `themes`                 |
| `contrast.ts`, `cssSafety.ts`                                                                                                                                          | `contrast`, `css_safety` |
| `motion.ts` (the `data-motion` decision)                                                                                                                               | `motion`                 |
| `backdrop.ts` (which OS effect a theme asks for, not the OS call)                                                                                                      | `backdrop`               |
| `:root` of `src/root.css` plus `var()`, `calc()`, `color-mix()`                                                                                                         | `css`                    |

`themes/*.json` and `src/root.css` are embedded with `include_str!`.

## API

```rust
use accshift_theme::{theme_application, ResolvedTheme, ThemeRegistry};

let mut registry = ThemeRegistry::new();
registry.apply_custom_theme_payloads(&payloads_from_disk);
let theme = registry.theme(Some(&settings.theme));
let applied = theme_application(&theme, settings.background_opacity, backdrop_available);
let resolved = ResolvedTheme::new(&applied);

let card = resolved.color("--bg-card");                  // Option<Rgba>
let radius = resolved.length_px("--radius-md");          // Option<f32>
let scale = resolved.number("--density-scale");          // Option<f32>
let entrance = resolved.duration_ms("--motion-card-entrance");
let shadow = resolved.shadow("--elevation-low");         // Option<Vec<Shadow>>
let ring = resolved.eval_color("color-mix(in srgb, var(--fg) 65%, transparent)");
let row = resolved.with_vars([("--row-custom-color", "#ff8800")]);
for item in resolved.unsupported_css() { log::warn!("theme CSS: {item}"); }
```

- `Rgba` is straight-alpha sRGB in f32 (0 to 1), so a `color-mix` result is not rounded before painting. `to_premultiplied_u8()` gives the layout of egui's `Color32`.
- `Length` keeps px, em, %, vw and vh apart (`calc(100% - 26px)` needs layout); `as_px()` returns the px value when nothing else is involved. `rem` is 16px.
- `ThemeApplication` is what `applyThemeToDocument` writes: `data-theme`, `data-glass`, `color-scheme`, the inline variables in order, the theme CSS, and the surface opacities.
- Cascade in `ResolvedTheme::new`, lowest first: `src/root.css`, the theme CSS, the inline variables, then the theme CSS `!important` declarations. Only `:root { --x: value }` declarations of a theme's CSS are applied; every other rule, at-rule or declaration is listed by `unsupported_css()`, never an error.
- `ThemeRegistry::prepare_save` and `finish_save` split `saveThemeDocument` around the disk write, which stays with the app.

## The dump

`tests/data/*.json` holds what the TypeScript engine wrote on `<html>` for the six built-in themes and five custom ones, each at background opacity 0, 50, 85 and 100, with the backdrop available and not. `tests/dump.rs` compares every variable of every case. The TypeScript is gone, so the dump is frozen: a deliberate change to the engine updates the expected values there.

## Known differences

- A document's tokens are a `BTreeMap<TokenKey, String>`: unknown keys cannot be stored, so `validate_theme_document` reports token issues in spec order and never reports `unknownToken`. Parsing still lists unknown keys in `rejected_tokens`.
- `var()` is substituted as text before tokenizing. A value that relies on token boundaries around a `var()` (`var(--n)px`) would parse where a browser refuses it.
- `color-mix` supports `in srgb` only, the one space the app uses. `min()`, `max()` and `clamp()` are not evaluated.
