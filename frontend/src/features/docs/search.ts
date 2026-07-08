import { ALL_PAGES } from "./content";

export type SearchResult = { pageId: string; title: string; sectionTitle: string; snippet: string; score: number };

/// Strip markdown syntax to plain text for matching + snippets (no `#`, `*`, backticks, etc.).
function plain(md: string): string {
  return md
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/[#>*`_|\-]+/g, " ")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/// A short snippet of body text centered on the first query hit, for the result row.
function snippetAround(text: string, term: string): string {
  const idx = text.toLowerCase().indexOf(term);
  if (idx < 0) return text.slice(0, 120);
  const start = Math.max(0, idx - 40);
  const end = Math.min(text.length, idx + term.length + 80);
  return (start > 0 ? "…" : "") + text.slice(start, end).trim() + (end < text.length ? "…" : "");
}

/// Client-side ranked full-text search over the docs. No dependency: term-AND matching with a
/// title/description weighted higher than body. Returns results sorted by score (desc).
export function searchDocs(query: string): SearchResult[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const terms = q.split(/\s+/).filter(Boolean);

  const results: SearchResult[] = [];
  for (const { section, page } of ALL_PAGES) {
    const title = page.title.toLowerCase();
    const desc = page.description.toLowerCase();
    const body = plain(page.body);
    const bodyLower = body.toLowerCase();

    // Every term must appear somewhere in the page (AND), else it's not a match.
    if (!terms.every((t) => title.includes(t) || desc.includes(t) || bodyLower.includes(t))) continue;

    let score = 0;
    for (const t of terms) {
      if (title.includes(t)) score += 10;
      if (desc.includes(t)) score += 4;
      // Count body occurrences (capped) so a page that mentions the term a lot ranks higher.
      const hits = bodyLower.split(t).length - 1;
      score += Math.min(hits, 5);
    }

    results.push({
      pageId: page.id,
      title: page.title,
      sectionTitle: section.title,
      snippet: snippetAround(body, terms[0]),
      score,
    });
  }
  return results.sort((a, b) => b.score - a.score);
}
