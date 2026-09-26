/**
 * The safety filter for a theme's custom CSS.
 *
 * Kept apart from the schema so the one piece that stands between a shared
 * file and a network request can be read, and tested, on its own.
 */

/**
 * Constructs a theme's custom CSS may not contain.
 *
 * CSS cannot run code here, but it can still reach out: a `url()` or an
 * `@import` turns applying a shared theme into a request that tells its author
 * the app started, and `</style` ends the tag and hands the rest to the HTML
 * parser. A theme is a file people pass around, so what it can do stops at
 * painting the window it is applied to.
 */
const FORBIDDEN_CSS: ReadonlyArray<{ label: string; pattern: RegExp }> = [
  { label: "@import", pattern: /@import\b/i },
  { label: "url()", pattern: /\burl\s*\(/i },
  // These take a bare string and fetch it just like url() does.
  { label: "image-set()", pattern: /image-set\s*\(/i },
  { label: "image()", pattern: /\bimage\s*\(/i },
  { label: "src()", pattern: /\bsrc\s*\(/i },
  { label: "expression()", pattern: /\bexpression\s*\(/i },
  { label: "javascript:", pattern: /javascript\s*:/i },
  { label: "-moz-binding", pattern: /-moz-binding/i },
  { label: "</style", pattern: /<\/\s*style/i },
];

// Strip comments and resolve escapes the way the CSS tokenizer does, so a
// comment in the middle of `url`, a hex escape (`\75rl(`) and a plain escape
// (`u\rl(`, `@imp\ort`) all match the construct a style tag would read. A
// backslash before a newline is dropped too: over-matching only costs a theme
// its CSS, under-matching lets it fetch.
function normalizeCssForSafety(css: string): string {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  return withoutComments.replace(
    /\\(?:([0-9a-fA-F]{1,6})(?:\r\n|[ \t\f\n\r])?|(\r\n|[\n\r\f])|([\s\S]))/g,
    (_, hex: string | undefined, _newline: string | undefined, char: string | undefined) => {
      if (hex !== undefined) {
        const code = Number.parseInt(hex, 16);
        if (code === 0 || code > 0x10ffff) return "";
        return String.fromCodePoint(code);
      }
      return char ?? "";
    },
  );
}

/** The construct that makes this CSS unusable, or null when it is clean. */
export function unsafeCssConstruct(css: string): string | null {
  const normalized = normalizeCssForSafety(css);
  return FORBIDDEN_CSS.find((entry) => entry.pattern.test(normalized))?.label ?? null;
}
