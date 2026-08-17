# Theming

A theme is a JSON file. It sets values called tokens, the app writes them to
CSS custom properties on the document root, and the interface repaints. There
is nothing to compile: themes are created and edited from the app, saved to
disk, and shared as a single file.

This document is the contract. It says what a theme file may contain, what
happens to a file that is incomplete or written for an older version of the
format, and what each token controls.

## Where themes live

Built-in themes ship inside the app. Every theme you create or import is one
`<id>.json` file in the `themes` folder of the app config directory, next to
the rest of the configuration. Deleting the file removes the theme, and the app
falls back to Dark if the deleted theme was the active one.

The theme folder is part of the backup manifest, so an app backup carries your
themes with it.

## The file

```json
{
  "schemaVersion": 2,
  "id": "nord",
  "name": "Nord",
  "author": "someone",
  "version": "1.0.0",
  "colorScheme": "dark",
  "extends": "dark",
  "glass": false,
  "tokens": {
    "accent": "#88c0d0",
    "bgCard": "#3b4252"
  }
}
```

| Field           | Required | Meaning                                                                               |
| --------------- | -------- | ------------------------------------------------------------------------------------- |
| `schemaVersion` | no       | Contract version the file was written for. Absent means 1.                            |
| `id`            | yes      | Stable identifier and file name. Letters, digits, `-` and `_`, 64 characters at most. |
| `name`          | yes      | What the theme is called in the picker. 64 characters at most.                        |
| `author`        | no       | Free text, shown nowhere but carried through export and import.                       |
| `version`       | no       | The theme author's own version string. Not the contract version.                      |
| `colorScheme`   | yes      | `dark` or `light`. Decides the root values and the native form controls.              |
| `extends`       | no       | Id of the theme this one inherits from.                                               |
| `glass`         | no       | `true` puts the theme on the translucent surface scale.                               |
| `tokens`        | yes      | Map of token name to value. Only the tokens the theme cares about.                    |
| `css`           | no       | Raw CSS applied on top of the theme. See Custom CSS below.                            |

Unknown fields are ignored. Unknown or malformed tokens are dropped, and the
rest of the file still applies.

## Versioning and migration

`schemaVersion` is the version of this contract, not of your theme. It is
bumped when a token is added, removed, or changes meaning.

- **1**: the eleven original colour tokens.
- **2**: accent and status colours, radii, elevations, density, interface font.

A file with no `schemaVersion` is a version 1 file. It is migrated on load by
being pointed at the built-in theme of its own colour scheme, so its eleven
colours keep winning and the tokens added since are inherited rather than
missing. Nothing is rewritten on disk until you save the theme from the editor.

A file that declares a version this build does not know is refused whole, with
a message saying it needs a newer accshift. Applying half of it would produce a
theme its author never wrote.

## Inheritance

`extends` names the theme to take every token this file does not set from. It
is what makes a theme a handful of lines instead of a full palette: the two
built-in themes loaded from JSON, `midnight` and `glass-dark`, each extend
`dark` and override only colours.

Resolution walks the chain nearest first, and the built-in root of the
document's colour scheme is always the last word. That root is complete by
construction, so resolution always produces a complete token set.

The chain is capped at eight documents. A cycle, a missing base or a chain that
runs past the cap stops the walk and lets the root fill the rest: the theme
still paints, and the editor says which base it could not find.

## Custom CSS

`css` holds raw CSS, applied after the app stylesheet when the theme is active
and removed the moment another theme is selected. It is there for what the
tokens cannot express, a spacing tweak or a rule on one specific element, and
it is the part of a theme most likely to break: the class names it targets are
internal and change between releases.

Custom CSS is not inherited through `extends`. A rule written against one
theme's markup has no reason to follow every theme that inherits from it.
Duplicating a theme copies its CSS instead.

Four constructs are refused, and a file carrying one applies with its CSS
dropped rather than being rejected whole:

| Refused        | Why                                                                  |
| -------------- | -------------------------------------------------------------------- |
| `@import`      | Fetches a remote stylesheet, which tells its author the app started. |
| `url()`        | Same, through a background image or a font.                          |
| `expression()` | Legacy Internet Explorer construct that evaluates script.            |
| `</style`      | Ends the tag and hands the rest of the file to the HTML parser.      |

`javascript:` and `-moz-binding` are refused for the same reason. The cap is
20 000 characters, and the theme file itself may not exceed 64 KB.

## Tokens

Every token is a string. The **CSS** column is the custom property it feeds, so
you can see what a value affects by searching the stylesheets for it.

### Surfaces

| Token         | CSS               | Kind        | Since | Role                                                                                 |
| ------------- | ----------------- | ----------- | ----- | ------------------------------------------------------------------------------------ |
| `bgRgb`       | `--bg-rgb`        | rgb triplet | 1     | Window fill. A triplet, not a colour, because the opacity slider supplies the alpha. |
| `bgCard`      | `--bg-card`       | hex colour  | 1     | Account cards, panels, settings sections.                                            |
| `bgCardHover` | `--bg-card-hover` | hex colour  | 1     | Same surfaces under the pointer.                                                     |
| `bgMuted`     | `--bg-muted`      | hex colour  | 1     | Recessed surfaces: inputs, empty states, badges.                                     |
| `bgElevated`  | `--bg-elevated`   | hex colour  | 1     | Surfaces raised above a card: menus, popovers.                                       |
| `border`      | `--border`        | colour      | 1     | Every separator and outline.                                                         |

The four `bg*` tokens are painted at an alpha the app computes from the theme
kind and the opacity slider, which is why they must be plain hex: the value is
split into channels before it reaches CSS.

### Text

| Token      | CSS           | Kind   | Since | Role                                  |
| ---------- | ------------- | ------ | ----- | ------------------------------------- |
| `fg`       | `--fg`        | colour | 1     | Primary text and icons.               |
| `fgMuted`  | `--fg-muted`  | colour | 1     | Secondary text: labels, descriptions. |
| `fgSubtle` | `--fg-subtle` | colour | 1     | Tertiary text: hints, disabled items. |
| `afkText`  | `--afk-text`  | colour | 1     | The away marker drawn over an avatar. |

### Status colours

| Token      | CSS           | Kind   | Since | Role                               |
| ---------- | ------------- | ------ | ----- | ---------------------------------- |
| `accent`   | `--accent`    | colour | 2     | Selection, focus, primary buttons. |
| `accentFg` | `--accent-fg` | colour | 2     | Text and icons drawn on `accent`.  |
| `success`  | `--success`   | colour | 2     | Confirmations and healthy states.  |
| `warning`  | `--warning`   | colour | 2     | Warnings and degraded states.      |
| `danger`   | `--danger`    | colour | 1     | Errors and destructive actions.    |

### Shape and density

| Token             | CSS                  | Kind   | Since | Role                                                                 |
| ----------------- | -------------------- | ------ | ----- | -------------------------------------------------------------------- |
| `radiusSm`        | `--radius-sm`        | length | 2     | Badges, inputs, small controls.                                      |
| `radiusMd`        | `--radius-md`        | length | 2     | Cards and panels.                                                    |
| `radiusLg`        | `--radius-lg`        | length | 2     | Dialogs and large containers.                                        |
| `elevationLow`    | `--elevation-low`    | shadow | 2     | Resting shadow.                                                      |
| `elevationMedium` | `--elevation-medium` | shadow | 2     | Hovered and floating surfaces.                                       |
| `elevationHigh`   | `--elevation-high`   | shadow | 2     | Dialogs and menus.                                                   |
| `density`         | `--density-scale`    | choice | 2     | `compact`, `cozy` or `comfortable`. Scales the account grid metrics. |

### Typography

| Token    | CSS         | Kind | Since | Role                                                                                                                                |
| -------- | ----------- | ---- | ----- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `fontUi` | `--font-ui` | font | 2     | Interface font. It is placed in front of the bundled stack, never in place of it, so the Cyrillic and Han fallbacks stay behind it. |

## Accepted values

Values are validated per kind. A value that fails is dropped, and the inherited
one is used in its place.

| Kind        | Accepted                                          | Example                       |
| ----------- | ------------------------------------------------- | ----------------------------- |
| rgb triplet | Three numbers 0 to 255, space separated           | `9 9 11`                      |
| hex colour  | `#rgb` or `#rrggbb`                               | `#1c1c1f`                     |
| colour      | Hex, `rgb()`, `rgba()`, or `white` and `black`    | `#fafafa`                     |
| length      | A number with `px`, `rem` or `em`, or bare `0`    | `8px`                         |
| shadow      | `none`, or shadow syntax: numbers, units, colours | `0 2px 8px rgb(0 0 0 / 0.18)` |
| choice      | One of the values the token lists                 | `cozy`                        |
| font        | Family names, quotes and commas                   | `"Inter", sans-serif`         |

Values are capped at 200 characters, and shadows may not contain `url(`,
`var(` or `expression(`. Theme files travel between users and their values are
written straight into custom properties, so a value is allowed to describe a
colour and nothing else.

## Contrast

The editor measures ten pairs with the WCAG 2.1 contrast formula and reports
anything below target. Each pair has a target, below which you get a warning,
and a floor, below which the text is unreadable rather than merely tight and
you get an error.

| Foreground | Background | Target | Floor |
| ---------- | ---------- | ------ | ----- |
| `fg`       | `bgRgb`    | 4.5    | 3     |
| `fg`       | `bgCard`   | 4.5    | 3     |
| `fgMuted`  | `bgCard`   | 4.5    | 3     |
| `fgSubtle` | `bgCard`   | 3      | 2     |
| `afkText`  | `bgCard`   | 3      | 2     |
| `danger`   | `bgCard`   | 3      | 2     |
| `success`  | `bgCard`   | 3      | 2     |
| `warning`  | `bgCard`   | 3      | 2     |
| `accent`   | `bgCard`   | 3      | 2     |
| `accentFg` | `accent`   | 4.5    | 3     |

On a glass theme the card surfaces are composited over the window fill at the
alpha they are really painted at before the ratio is computed. Measuring the
raw token would score a surface nobody ever sees: Liquid Glass cards are white
at 13 percent over a dark window, which reads as dark.

## What a broken theme does

Nothing that a theme file can contain takes the interface down.

- A token that is missing anywhere in the chain comes from the built-in root.
- A value that fails validation is dropped, and the inherited value is used.
- A token name that is not in this document is ignored.
- A base that does not exist, or a cycle, resolves against the root instead.
- A file that is not JSON, has no usable `id` or no `name`, is skipped at
  startup and refused with a message on import.
- A file declaring a contract version this build does not know is refused
  whole.
- Custom CSS holding a refused construct is dropped; the tokens still apply.

Files that fail at startup are skipped silently, because they were already on
disk before you did anything and a dialog at launch helps nobody. The editor
and the import path report the same failures out loud.

## Editing in the app

Settings, Appearance, under the theme picker. The editor is marked beta: the
contract is still moving, and a theme written today may need a pass after an
update.

- **Customize** copies the selected theme into a new one that extends it, so
  you start from a built-in without duplicating its palette. On a theme you
  already own the same button reads **Edit**.
- Every change is applied to the running interface as you make it. Cancel puts
  back exactly what was on screen, including the surface opacities.
- Each token row shows the value in force. A row you have not touched is marked
  as inherited and names where the value comes from; the reset button next to a
  row you have touched drops your override and gives the inherited value back.
- The checks panel lists everything wrong with the theme, worst first. Refused
  values block saving, because they would be dropped on the next load and the
  theme would silently differ. Contrast problems do not block: they warn.
- The custom CSS field takes raw CSS and applies it as you type. A refused
  construct blocks saving, for the same reason a refused value does.
- **Export** copies the theme to the clipboard as one self-contained file,
  metadata included. **Import** reads one back from the clipboard.

## Adding a token

For contributors. Adding a token is a contract change, so it takes five steps:

1. Bump `THEME_CONTRACT_VERSION` in `src/lib/theme/tokens.ts` and add the new
   version to the list in this document.
2. Add the field to `ThemeTokens` and its entry to `THEME_TOKEN_SPECS`, with
   the `since` of the new version.
3. Give both roots a value in `src/lib/theme/themes.ts`. The test suite fails
   if either root is incomplete.
4. Write the property in `applyThemeToDocument` and give it a fallback in
   `src/app.css`, so a document rendered before the theme is applied still has
   a value.
5. Add the `themeToken.<key>` label to every dictionary in `src/lib/i18n`, and
   its row to the token reference above.

Older files keep working without a migration entry as long as the resolver can
fill the new token from the root, which is exactly what version 1 files get.
Removing or repurposing a token is what needs real migration work.
