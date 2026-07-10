const DIACRITICS = /[̀-ͯ]/g;

export function foldText(text: string): string {
  return text.normalize("NFD").replace(DIACRITICS, "").toLowerCase();
}

/** Subsequence scorer: every query char must appear in order. Consecutive
 *  matches and word starts score higher, so "nf" ranks "New folder" above
 *  "info". Returns null when the query is not a subsequence. */
export function fuzzyScore(query: string, text: string): number | null {
  const q = foldText(query);
  const t = foldText(text);
  if (!q) return 0;
  let score = 0;
  let ti = 0;
  let lastMatch = -2;
  for (let qi = 0; qi < q.length; qi++) {
    const ch = q[qi];
    const found = t.indexOf(ch, ti);
    if (found === -1) return null;
    if (found === lastMatch + 1) score += 6;
    if (found === 0 || t[found - 1] === " " || t[found - 1] === "-") score += 8;
    score += 1;
    // Small penalty for skipped distance keeps tight matches on top.
    score -= Math.min(found - ti, 4);
    lastMatch = found;
    ti = found + 1;
  }
  // Shorter targets win ties ("Steam" over "Steam launch options").
  score += Math.max(0, 12 - Math.floor(t.length / 4));
  return score;
}

/** Best score across a title and optional keywords. */
export function fuzzyScoreAll(query: string, title: string, keywords?: string[]): number | null {
  let best = fuzzyScore(query, title);
  if (keywords) {
    for (const keyword of keywords) {
      const s = fuzzyScore(query, keyword);
      // Keyword hits rank slightly below equivalent title hits.
      if (s !== null && (best === null || s - 2 > best)) best = s - 2;
    }
  }
  return best;
}
