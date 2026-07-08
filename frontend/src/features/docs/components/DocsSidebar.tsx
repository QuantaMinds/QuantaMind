import { useState } from "react";
import { DOC_SECTIONS } from "../content";

/// The left nav tree: collapsible sections, each listing its pages. The active page is
/// highlighted. Mirrors a docs-site sidebar.
export function DocsSidebar({ activeId, onSelect }: { activeId: string; onSelect: (pageId: string) => void }) {
  // All sections start expanded; the user can collapse any.
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const toggle = (id: string) => setCollapsed((c) => ({ ...c, [id]: !c[id] }));

  return (
    <nav className="w-56 shrink-0 border-r border-slate-200 overflow-y-auto pr-2" data-testid="docs-sidebar" aria-label="Docs navigation">
      {DOC_SECTIONS.map((section) => {
        const isCollapsed = collapsed[section.id] ?? false;
        return (
          <div key={section.id} className="mb-3">
            <button
              type="button"
              onClick={() => toggle(section.id)}
              className="w-full flex items-center gap-1 px-2 py-1 text-[11px] font-semibold uppercase tracking-wide text-slate-400 hover:text-slate-600"
              aria-expanded={!isCollapsed}
            >
              <span className={`inline-block transition-transform ${isCollapsed ? "-rotate-90" : ""}`}>▾</span>
              {section.title}
            </button>
            {!isCollapsed && (
              <ul className="mt-0.5">
                {section.pages.map((page) => {
                  const active = page.id === activeId;
                  return (
                    <li key={page.id}>
                      <button
                        type="button"
                        onClick={() => onSelect(page.id)}
                        aria-current={active ? "page" : undefined}
                        data-testid={`docs-nav-${page.id}`}
                        className={`w-full text-left pl-6 pr-2 py-1.5 text-[13px] rounded-md ${
                          active ? "bg-blue-50 text-blue-700 font-medium" : "text-slate-600 hover:bg-slate-50"
                        }`}
                      >
                        {page.title}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        );
      })}
    </nav>
  );
}
