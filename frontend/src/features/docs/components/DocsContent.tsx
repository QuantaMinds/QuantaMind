import { useMemo, useRef } from "react";
import type { LocatedPage } from "../content";
import { DocMarkdown, tocFromMarkdown } from "../render";

/// The center content column + right-rail "On this page" table of contents. The article scrolls;
/// TOC entries scroll to their heading within it. `contentKey` lets the parent reset scroll on
/// page change.
export function DocsContent({ located }: { located: LocatedPage }) {
  const { section, page } = located;
  const articleRef = useRef<HTMLDivElement>(null);
  const toc = useMemo(() => tocFromMarkdown(page.body), [page.body]);

  const scrollTo = (id: string) => {
    // Scope to THIS article so a slug can't collide with an always-mounted sibling tab.
    const el = articleRef.current?.querySelector(`#${CSS.escape(id)}`);
    el?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <div className="flex flex-1 min-h-0 gap-6" data-testid="docs-content">
      <article ref={articleRef} key={page.id} className="flex-1 min-w-0 overflow-y-auto pr-4 pb-16">
        <div className="text-[11px] text-slate-400 mb-2" data-testid="docs-breadcrumb">
          {section.title} <span className="mx-1">›</span> {page.title}
        </div>
        <DocMarkdown markdown={page.body} />
      </article>

      {toc.length > 0 && (
        <aside className="w-48 shrink-0 overflow-y-auto hidden lg:block" aria-label="On this page">
          <div className="text-[11px] font-semibold uppercase tracking-wide text-slate-400 mb-2">On this page</div>
          <ul className="space-y-1.5 border-l border-slate-200">
            {toc.map((t) => (
              <li key={t.id} style={{ paddingLeft: t.level === 3 ? 20 : 10 }}>
                <button
                  type="button"
                  onClick={() => scrollTo(t.id)}
                  className="text-left text-[12px] text-slate-500 hover:text-blue-600 -ml-px border-l border-transparent hover:border-blue-400 pl-2"
                  data-testid={`docs-toc-${t.id}`}
                >
                  {t.title}
                </button>
              </li>
            ))}
          </ul>
        </aside>
      )}
    </div>
  );
}
