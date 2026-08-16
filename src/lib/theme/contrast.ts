/**
 * WCAG 2.1 contrast, used by the theme editor to tell a user that a colour
 * they just picked makes text unreadable. Only the colour notations a theme
 * token may hold are parsed (hex, rgb(), a few keywords); anything else
 * returns null and the check is skipped rather than guessed.
 */

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

const KEYWORDS: Record<string, Rgb> = {
  white: { r: 255, g: 255, b: 255 },
  black: { r: 0, g: 0, b: 0 },
};

export function parseColor(value: string): Rgb | null {
  const raw = value.trim();
  if (!raw) return null;

  if (raw.startsWith("#")) {
    const hex = raw.slice(1);
    const normalized =
      hex.length === 3
        ? hex
            .split("")
            .map((char) => `${char}${char}`)
            .join("")
        : hex;
    if (!/^[0-9a-fA-F]{6}$/.test(normalized)) return null;
    return {
      r: Number.parseInt(normalized.slice(0, 2), 16),
      g: Number.parseInt(normalized.slice(2, 4), 16),
      b: Number.parseInt(normalized.slice(4, 6), 16),
    };
  }

  const fn = raw.match(/^rgba?\(([^)]+)\)$/);
  if (fn) {
    const parts = fn[1]
      .split(/[\s,/]+/)
      .filter(Boolean)
      .map((part) => Number.parseFloat(part));
    if (parts.length < 3 || parts.slice(0, 3).some((part) => Number.isNaN(part))) return null;
    return { r: parts[0], g: parts[1], b: parts[2] };
  }

  // "9 9 11": the triplet notation the window fill token uses.
  const triplet = raw.split(/\s+/);
  if (triplet.length === 3 && triplet.every((part) => /^\d{1,3}$/.test(part))) {
    return { r: Number(triplet[0]), g: Number(triplet[1]), b: Number(triplet[2]) };
  }

  return KEYWORDS[raw.toLowerCase()] ?? null;
}

/** Source colour painted at `alpha` over an opaque backdrop. */
export function blend(source: Rgb, alpha: number, backdrop: Rgb): Rgb {
  const a = Math.min(1, Math.max(0, alpha));
  return {
    r: source.r * a + backdrop.r * (1 - a),
    g: source.g * a + backdrop.g * (1 - a),
    b: source.b * a + backdrop.b * (1 - a),
  };
}

function channelLuminance(channel: number): number {
  const c = Math.min(255, Math.max(0, channel)) / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

export function relativeLuminance(color: Rgb): number {
  return (
    0.2126 * channelLuminance(color.r) +
    0.7152 * channelLuminance(color.g) +
    0.0722 * channelLuminance(color.b)
  );
}

/** WCAG contrast ratio, 1 (identical) to 21 (black on white). */
export function contrastRatio(a: Rgb, b: Rgb): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  const lighter = Math.max(la, lb);
  const darker = Math.min(la, lb);
  return (lighter + 0.05) / (darker + 0.05);
}

/** Rounded to one decimal, which is how the editor prints it. */
export function roundRatio(ratio: number): number {
  return Math.round(ratio * 10) / 10;
}
