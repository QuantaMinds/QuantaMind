import { useEffect, useState } from "react";
import { useNavStore } from "../../../shared/state/navStore";
import { useHotkey } from "../../../shared/ui/useHotkey";
import { DEFAULT_PAGE_ID, findPage } from "../content";
import { DocsSidebar } from "./DocsSidebar";
import { DocsContent } from "./DocsContent";
import { DocsSearch } from "./DocsSearch";

/// A `#docs-<pageId>` deep link, optionally with a `--<anchorSlug>` suffix to scroll to a heading
/// within the page (`#docs-<pageId>--<slug>`). `--` is the separator; heading slugs never contain
/// one. Returns the valid page id + optional anchor, or null.
function hashTarget(): { pageId: string; anchor: string | null } | null {
  const m = /^#docs-(.+)$/.exec(window.location.hash);
  if (!m) return null;
  const [pageId, anchor] = m[1].split("--", 2);
  return findPage(pageId) ? { pageId, anchor: anchor ?? null } : null;
}

/// Scroll to a heading anchor, scoped to the Docs subtree so a slug can't match an always-mounted
/// sibling tab. Deferred a tick so the target page has rendered first (mirrors the old Help scroll).
function scrollToAnchor(slug: string) {
  setTimeout(() => {
    const root = document.querySelector('[data-testid="page-docs"]');
    root?.querySelector(`#${CSS.escape(slug)}`)?.scrollIntoView({ block: "start" });
  }, 0);
}

/// The Docs tab: a docs-site layout — collapsible sidebar nav, center content, right-rail TOC,
/// and ⌘K search. The friendly, task-oriented user guide (distinct from the Help tab's per-feature
/// reference).
export function DocsPage() {
  const isActive = useNavStore((s) => s.topView === "docs");
  const [pageId, setPageId] = useState<string>(() => hashTarget()?.pageId ?? DEFAULT_PAGE_ID);
  const [searchOpen, setSearchOpen] = useState(false);

  // Deep-link both ways: react to hash changes (honoring an optional `--<anchor>` to scroll to a
  // block, e.g. the CSV-import "Learn more" link), and stamp the hash when a page is chosen (while
  // Docs is active) so links + back/forward work.
  useEffect(() => {
    const onHash = () => {
      const t = hashTarget();
      if (!t) return;
      setPageId(t.pageId);
      if (t.anchor) scrollToAnchor(t.anchor);
    };
    onHash();
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const goTo = (id: string) => {
    setPageId(id);
    if (window.location.hash !== `#docs-${id}`) window.location.hash = `docs-${id}`;
  };

  // ⌘K / Ctrl+K opens search — only while the Docs tab is the active view (the page is always
  // mounted but hidden), so it doesn't hijack the shortcut on other tabs.
  useHotkey("mod+k", () => setSearchOpen(true), isActive);

  const located = findPage(pageId) ?? findPage(DEFAULT_PAGE_ID)!;

  return (
    <section data-testid="page-docs" className="flex flex-col gap-3 h-full">
      <header className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold">Docs</h2>
          <p className="text-xs text-gray-600">Guides for getting the most out of QuantaMind.</p>
        </div>
        <button
          type="button"
          onClick={() => setSearchOpen(true)}
          data-testid="docs-search-trigger"
          className="flex items-center gap-2 text-xs text-slate-500 border border-slate-200 rounded-md px-3 py-1.5 hover:bg-slate-50"
        >
          <span>🔍 Search the docs</span>
          <kbd className="text-[10px] text-slate-400 border border-slate-200 rounded px-1.5 py-0.5">⌘K</kbd>
        </button>
      </header>

      <div className="flex flex-1 min-h-0 gap-4">
        <DocsSidebar activeId={pageId} onSelect={goTo} />
        <DocsContent located={located} />
      </div>

      <DocsSearch open={searchOpen} onClose={() => setSearchOpen(false)} onNavigate={goTo} />
    </section>
  );
}
