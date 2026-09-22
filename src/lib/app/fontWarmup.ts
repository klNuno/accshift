/**
 * Which fonts the first frame used, for the next start to open them early.
 *
 * Most of the first layout of the account grid went to opening font files:
 * each weight of the interface face, then for names with Han characters or
 * emoji the fallback faces behind them, tens of megabytes each. WebView2's
 * renderer keeps the font files it has opened in a cache that all its threads
 * share, so index.html hands this hint to /font-warmup.js, a worker that
 * measures the same text in the same fonts while the page is still booting.
 * The layout then finds them open, which halved it on a grid with Han and
 * emoji names.
 *
 * Only called on Windows: WebView2 is where that shared cache was measured.
 */

// Read by the head script in index.html under the same name.
const FONT_WARMUP_KEY = "accshift_font_warmup";

// Enough for every face on the main screen; the worker takes at most 16.
const MAX_FONTS = 12;
// The characters only pick fallback faces, so one per face would do. A cap
// keeps a grid of long foreign names from growing the hint.
const MAX_CHARS = 64;
// Below this, the Latin blocks: the interface face has them, and measuring
// "Aa" already opens it.
const FIRST_FALLBACK_CODE_POINT = 0x250;

export function recordFontWarmup(root: Element): void {
  try {
    const fonts = new Set<string>();
    const chars = new Set<string>();
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.nodeValue;
      const parent = node.parentElement;
      if (!text?.trim() || !parent?.checkVisibility()) continue;
      if (fonts.size < MAX_FONTS) {
        const style = getComputedStyle(parent);
        fonts.add(`${style.fontStyle} ${style.fontWeight} 16px ${style.fontFamily}`);
      }
      for (const char of text) {
        if (chars.size >= MAX_CHARS) break;
        if (char.codePointAt(0)! >= FIRST_FALLBACK_CODE_POINT) chars.add(char);
      }
    }
    if (fonts.size === 0) {
      localStorage.removeItem(FONT_WARMUP_KEY);
      return;
    }
    localStorage.setItem(
      FONT_WARMUP_KEY,
      JSON.stringify({ fonts: [...fonts], text: `Aa${[...chars].join("")}` }),
    );
  } catch {
    // Storage off or full: the next start loads its fonts at layout.
  }
}
