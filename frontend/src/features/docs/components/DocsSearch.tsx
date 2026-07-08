import { useEffect, useMemo, useRef, useState } from "react";
import { searchDocs } from "../search";

/// The ⌘K / Ctrl+K command-palette search modal. Client-side ranked search over the docs;
/// arrow keys move the selection, Enter opens it, Escape / outside-click closes.
export function DocsSearch({ open, onClose, onNavigate }: {
  open: boolean;
  onClose: () => void;
  onNavigate: (pageId: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const results = useMemo(() => searchDocs(query), [query]);

  // Reset + focus each time it opens.
  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
      // Focus after paint so the input exists.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => setActive(0), [query]);

  if (!open) return null;

  const choose = (pageId: string) => {
    onNavigate(pageId);
    onClose();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") { onClose(); return; }
    if (e.key === "ArrowDown") { e.preventDefault(); setActive((a) => Math.min(a + 1, results.length - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => Math.max(a - 1, 0)); }
    else if (e.key === "Enter" && results[active]) { e.preventDefault(); choose(results[active].pageId); }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-24 bg-black/30"
      onClick={onClose}
      data-testid="docs-search-overlay"
    >
      <div
        role="dialog"
        aria-label="Search docs"
        className="bg-white rounded-xl shadow-2xl w-[32rem] max-w-[90vw] max-h-[70vh] overflow-hidden flex flex-col"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
        data-testid="docs-search-modal"
      >
        <div className="flex items-center gap-2 border-b border-slate-200 px-3">
          <span className="text-slate-400 text-sm">🔍</span>
          <input
            ref={inputRef}
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search the docs…"
            className="flex-1 py-3 text-sm outline-none bg-transparent"
            data-testid="docs-search-input"
          />
          <kbd className="text-[10px] text-slate-400 border border-slate-200 rounded px-1.5 py-0.5">esc</kbd>
        </div>
        <div className="overflow-y-auto">
          {query.trim() === "" ? (
            <p className="px-4 py-6 text-xs text-slate-400 text-center">Type to search across all guides.</p>
          ) : results.length === 0 ? (
            <p className="px-4 py-6 text-xs text-slate-400 text-center" data-testid="docs-search-empty">No matches for "{query}".</p>
          ) : (
            <ul>
              {results.map((r, i) => (
                <li key={r.pageId}>
                  <button
                    type="button"
                    onMouseEnter={() => setActive(i)}
                    onClick={() => choose(r.pageId)}
                    className={`w-full text-left px-4 py-2.5 border-b border-slate-50 ${i === active ? "bg-blue-50" : "hover:bg-slate-50"}`}
                    data-testid="docs-search-result"
                  >
                    <div className="flex items-baseline gap-2">
                      <span className="text-sm font-medium text-slate-800">{r.title}</span>
                      <span className="text-[10px] uppercase tracking-wide text-slate-400">{r.sectionTitle}</span>
                    </div>
                    <div className="text-[11px] text-slate-500 truncate">{r.snippet}</div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
