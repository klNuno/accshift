import type { KeyScope, ParsedCombo, ShortcutBinding } from "./types";

type KeyboardControllerDeps = {
  getScope: () => KeyScope;
  isMac: () => boolean;
  bindings: ShortcutBinding[];
};

export function parseCombo(combo: string): ParsedCombo {
  const parts = combo.toLowerCase().split("+");
  const key = parts[parts.length - 1];
  return {
    mod: parts.includes("mod"),
    shift: parts.includes("shift"),
    alt: parts.includes("alt"),
    key,
  };
}

export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

function eventMatchesKey(e: KeyboardEvent, key: string): boolean {
  if (key.startsWith("digit")) return e.code.toLowerCase() === key;
  if (key === "plus") return e.code === "Equal" || e.code === "NumpadAdd";
  if (key === "minus") return e.code === "Minus" || e.code === "NumpadSubtract";
  if (key === "space") return e.key === " ";
  return e.key.toLowerCase() === key;
}

function comboMatches(e: KeyboardEvent, parsed: ParsedCombo, isMac: boolean): boolean {
  const modPressed = isMac ? e.metaKey : e.ctrlKey;
  const strayMod = isMac ? e.ctrlKey : e.metaKey;
  if (parsed.mod !== modPressed) return false;
  if (!parsed.mod && strayMod) return false;
  if (parsed.shift !== e.shiftKey) return false;
  if (parsed.alt !== e.altKey) return false;
  return eventMatchesKey(e, parsed.key);
}

/** Central keyboard dispatcher: one window keydown listener in the capture
 *  phase so it arbitrates before every component-level svelte:window handler.
 *  Bindings are matched against the current scope; the first match wins and
 *  the event is stopped, which is what fixes the historical "Escape closes
 *  the dialog AND the bulk edit bar" double-close. */
export function createKeyboardController({ getScope, isMac, bindings }: KeyboardControllerDeps) {
  const parsed = bindings.map((binding) => ({
    binding,
    combo: parseCombo(binding.combo),
  }));

  function handleKeydown(e: KeyboardEvent) {
    const scope = getScope();
    const editable = isEditableTarget(e.target);
    for (const { binding, combo } of parsed) {
      if (!binding.scopes.includes("*") && !binding.scopes.includes(scope)) continue;
      if (!comboMatches(e, combo, isMac())) continue;
      const hasModifier = combo.mod || combo.alt;
      if (editable && !hasModifier && !binding.allowInInput) continue;
      const handled = binding.run(e);
      if (handled === false) continue;
      if (binding.preventDefault !== false) {
        e.preventDefault();
        e.stopPropagation();
      }
      return;
    }
  }

  function attach(): () => void {
    window.addEventListener("keydown", handleKeydown, { capture: true });
    return () => {
      window.removeEventListener("keydown", handleKeydown, { capture: true });
    };
  }

  return { attach };
}
