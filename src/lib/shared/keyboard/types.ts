import type { PASS } from "./controller";

export type KeyScope =
  | "locked"
  | "onboarding"
  | "palette"
  | "dialog"
  | "context-menu"
  | "bulk-edit"
  | "settings"
  | "personas"
  | "app";

export type ShortcutBinding = {
  /** Combo string, e.g. "mod+k", "f2", "delete", "mod+shift+n", "alt+arrowleft".
   *  "mod" is Ctrl on Windows/Linux and Cmd on macOS. Digits match by physical
   *  key (e.code DigitN) so they work on layouts where digits are shifted. */
  combo: string;
  /** Scopes where the binding fires. "*" matches every scope. */
  scopes: (KeyScope | "*")[];
  /** Single keys (no modifier) are skipped while an editable element has
   *  focus unless this is set. Modifier combos run there too, unless
   *  skipInInput is set. */
  allowInInput?: boolean;
  /** Skip the binding, modifier combo included, while an editable element has
   *  focus, so the field keeps its native behavior (Ctrl+A selects text). */
  skipInInput?: boolean;
  /** Defaults to true. Set false for observe-only bindings. */
  preventDefault?: boolean;
  /** Return PASS to signal "not handled here": the next matching binding is
   *  tried and the event is left untouched so legacy component-level
   *  listeners still see it. Any other result counts as handled. The return
   *  type rejects `() => (flag = false)`: write a block body instead. */
  run: (e: KeyboardEvent) => void | typeof PASS;
};

export type ParsedCombo = {
  mod: boolean;
  shift: boolean;
  alt: boolean;
  /** Normalized key name (lowercased e.key), or "digit1".."digit9" for
   *  physical-key digit matching, or "plus"/"minus" for zoom keys. */
  key: string;
};
