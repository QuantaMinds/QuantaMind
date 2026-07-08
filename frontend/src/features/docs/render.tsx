import type { ReactNode } from "react";
import { open } from "@tauri-apps/plugin-shell";

/// A docs-tuned markdown renderer (larger prose than the release-note `Markdown`), rendered
/// INERTLY — pure React nodes, never innerHTML/execute (CLAUDE.md rule 3e). Links route through
/// the Tauri shell rather than navigating the webview. Supports the subset the guides use:
/// headings (anchored), ordered/unordered lists, fenced code blocks, Tip/Note/Warning callouts,
/// and pipe tables.

export type TocEntry = { id: string; title: string; level: number };

/// A URL-safe id from a heading, used for the right-rail TOC + `#docs-…` deep links.
export function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "");
}

const INLINE = /(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)]+\))/g;

function inline(text: string): ReactNode[] {
  return text.split(INLINE).map((p, i) => {
    if (p.startsWith("**") && p.endsWith("**")) return <strong key={i} className="font-semibold text-slate-900">{p.slice(2, -2)}</strong>;
    if (p.startsWith("`") && p.endsWith("`")) {
      return <code key={i} className="px-1.5 py-0.5 bg-slate-100 rounded text-[12px] font-mono text-slate-800">{p.slice(1, -1)}</code>;
    }
    const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(p);
    if (link) {
      const href = link[2];
      // In-page anchors navigate the TOC; external links open in the OS browser.
      const isAnchor = href.startsWith("#");
      return (
        <a
          key={i}
          href={href}
          onClick={isAnchor ? undefined : (e) => { e.preventDefault(); void open(href); }}
          className="text-blue-600 hover:underline"
        >
          {link[1]}
        </a>
      );
    }
    return p;
  });
}

type Block =
  | { kind: "heading"; level: number; text: string; id: string }
  | { kind: "code"; lang: string; content: string }
  | { kind: "callout"; variant: string; lines: string[] }
  | { kind: "table"; header: string[]; rows: string[][] }
  | { kind: "ul"; items: string[] }
  | { kind: "ol"; items: string[] }
  | { kind: "p"; text: string };

const CALLOUT = /^>\s*\[!(TIP|NOTE|WARNING|IMPORTANT)\]\s*(.*)$/i;

/// Parse markdown into block structures. Line-oriented with lookahead for fences/tables/callouts.
function parseBlocks(md: string): Block[] {
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];

    // Fenced code block
    const fence = /^```(\w*)\s*$/.exec(line);
    if (fence) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !/^```\s*$/.test(lines[i])) { body.push(lines[i]); i++; }
      i++; // consume closing fence
      blocks.push({ kind: "code", lang: fence[1], content: body.join("\n") });
      continue;
    }

    // Callout (GitHub-style admonition)
    const call = CALLOUT.exec(line);
    if (call) {
      const bodyLines: string[] = [];
      if (call[2].trim()) bodyLines.push(call[2].trim());
      i++;
      while (i < lines.length && lines[i].startsWith(">")) { bodyLines.push(lines[i].replace(/^>\s?/, "")); i++; }
      blocks.push({ kind: "callout", variant: call[1].toUpperCase(), lines: bodyLines });
      continue;
    }

    // Pipe table: a `|` row followed by a `---|---` separator
    if (line.includes("|") && i + 1 < lines.length && /^\s*\|?[\s:|-]+\|?\s*$/.test(lines[i + 1]) && lines[i + 1].includes("-")) {
      const cells = (row: string) => row.replace(/^\s*\|/, "").replace(/\|\s*$/, "").split("|").map((c) => c.trim());
      const header = cells(line);
      i += 2; // header + separator
      const rows: string[][] = [];
      while (i < lines.length && lines[i].includes("|") && lines[i].trim() !== "") { rows.push(cells(lines[i])); i++; }
      blocks.push({ kind: "table", header, rows });
      continue;
    }

    // Heading
    const h = /^(#{1,4})\s+(.*)$/.exec(line);
    if (h) {
      const text = h[2].trim();
      blocks.push({ kind: "heading", level: h[1].length, text, id: slugify(text) });
      i++;
      continue;
    }

    // Ordered list
    if (/^\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\d+\.\s+/.test(lines[i])) { items.push(lines[i].replace(/^\d+\.\s+/, "")); i++; }
      blocks.push({ kind: "ol", items });
      continue;
    }

    // Unordered list
    if (/^[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^[-*]\s+/.test(lines[i])) { items.push(lines[i].replace(/^[-*]\s+/, "")); i++; }
      blocks.push({ kind: "ul", items });
      continue;
    }

    // Blank line
    if (line.trim() === "") { i++; continue; }

    // Paragraph: accumulate consecutive text lines
    const para: string[] = [line];
    i++;
    while (
      i < lines.length && lines[i].trim() !== "" &&
      !/^(#{1,4}\s|```|[-*]\s|\d+\.\s|>\s*\[!)/.test(lines[i]) &&
      !(lines[i].includes("|") && i + 1 < lines.length && /^\s*\|?[\s:|-]+\|?\s*$/.test(lines[i + 1]))
    ) { para.push(lines[i]); i++; }
    blocks.push({ kind: "p", text: para.join(" ") });
  }
  return blocks;
}

/// The level-2/3 headings of a doc, for the right-rail "On this page".
export function tocFromMarkdown(md: string): TocEntry[] {
  return parseBlocks(md)
    .filter((b): b is Extract<Block, { kind: "heading" }> => b.kind === "heading" && (b.level === 2 || b.level === 3))
    .map((b) => ({ id: b.id, title: b.text, level: b.level }));
}

const HEADING_CLASS: Record<number, string> = {
  1: "text-2xl font-bold text-slate-900 mt-2 mb-4",
  2: "text-xl font-semibold text-slate-900 mt-8 mb-3 scroll-mt-24",
  3: "text-base font-semibold text-slate-800 mt-6 mb-2 scroll-mt-24",
  4: "text-sm font-semibold text-slate-700 mt-4 mb-1 scroll-mt-24",
};

const CALLOUT_STYLE: Record<string, { box: string; label: string; title: string }> = {
  TIP: { box: "border-emerald-300 bg-emerald-50", label: "text-emerald-700", title: "Tip" },
  NOTE: { box: "border-blue-300 bg-blue-50", label: "text-blue-700", title: "Note" },
  IMPORTANT: { box: "border-violet-300 bg-violet-50", label: "text-violet-700", title: "Important" },
  WARNING: { box: "border-amber-300 bg-amber-50", label: "text-amber-700", title: "Warning" },
};

export function DocMarkdown({ markdown }: { markdown: string }) {
  const blocks = parseBlocks(markdown);
  return (
    <div className="text-[13px] leading-relaxed text-slate-700 max-w-3xl" data-testid="doc-markdown">
      {blocks.map((b, i) => {
        switch (b.kind) {
          case "heading": {
            const Tag = (`h${Math.min(b.level, 4)}` as "h1" | "h2" | "h3" | "h4");
            return <Tag key={i} id={b.id} className={HEADING_CLASS[b.level]}>{inline(b.text)}</Tag>;
          }
          case "code":
            return (
              <pre key={i} className="bg-slate-900 text-slate-100 rounded-lg p-3.5 my-4 overflow-x-auto text-[12px] font-mono leading-relaxed" data-testid="doc-code">
                <code>{b.content}</code>
              </pre>
            );
          case "callout": {
            const s = CALLOUT_STYLE[b.variant] ?? CALLOUT_STYLE.NOTE;
            return (
              <div key={i} className={`border-l-4 rounded-r-md px-4 py-3 my-4 ${s.box}`} data-testid="doc-callout">
                <div className={`text-[11px] font-semibold uppercase tracking-wide mb-1 ${s.label}`}>{s.title}</div>
                <div className="space-y-1">{b.lines.map((l, j) => <p key={j}>{inline(l)}</p>)}</div>
              </div>
            );
          }
          case "table":
            return (
              <div key={i} className="overflow-x-auto my-4">
                <table className="w-full text-left border-collapse text-[12px]">
                  <thead>
                    <tr className="border-b border-slate-300">
                      {b.header.map((c, j) => <th key={j} className="py-2 pr-4 font-semibold text-slate-800">{inline(c)}</th>)}
                    </tr>
                  </thead>
                  <tbody>
                    {b.rows.map((row, r) => (
                      <tr key={r} className="border-b border-slate-100">
                        {row.map((c, j) => <td key={j} className="py-2 pr-4 align-top">{inline(c)}</td>)}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          case "ul":
            return <ul key={i} className="my-3 space-y-1.5 pl-5 list-disc marker:text-slate-400">{b.items.map((it, j) => <li key={j}>{inline(it)}</li>)}</ul>;
          case "ol":
            return <ol key={i} className="my-3 space-y-1.5 pl-5 list-decimal marker:text-slate-400">{b.items.map((it, j) => <li key={j} className="pl-1">{inline(it)}</li>)}</ol>;
          case "p":
            return <p key={i} className="my-3">{inline(b.text)}</p>;
        }
      })}
    </div>
  );
}
