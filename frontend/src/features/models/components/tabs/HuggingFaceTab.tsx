import { useEffect, useRef, useState } from "react";
import { hfSearch, type HfSearchHit } from "../../../../shared/ipc/models/hf_browse";
import { formatIpcError } from "../../../../shared/ipc/core/error";
import { HuggingFaceRepoDetail } from "../HuggingFaceRepoDetail";
import { useModelStore } from "../../state/modelStore";

const DEBOUNCE_MS = 300;
type Status = "idle" | "loading" | "ready" | "error";

export function HuggingFaceTab() {
  const query = useModelStore((s) => s.hfSearchQuery);
  const setQuery = useModelStore((s) => s.setHfSearchQuery);
  const selected = useModelStore((s) => s.hfSelectedRepo);
  const setSelected = useModelStore((s) => s.setHfSelectedRepo);
  const [hits, setHits] = useState<HfSearchHit[]>([]);
  const [status, setStatus] = useState<Status>("idle");
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    const q = query.trim();
    if (!q) { setHits([]); setStatus("idle"); setError(null); return; }
    const me = ++seq.current;
    setStatus("loading"); setError(null);
    const t = setTimeout(() => {
      hfSearch(q, 30)
        .then((out) => {
          if (seq.current !== me) return;
          setHits(out); setStatus("ready");
        })
        .catch((e) => {
          if (seq.current !== me) return;
          setError(formatIpcError(e)); setStatus("error");
        });
    }, DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [query]);

  if (selected) {
    return <HuggingFaceRepoDetail repo={selected} onBack={() => setSelected(null)} />;
  }
  return (
    <div data-testid="tab-huggingface" className="flex flex-col gap-3 h-full">
      <div className="flex items-center gap-2">
        <input
          type="search"
          aria-label="Search Hugging Face"
          placeholder="Search Hugging Face models…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="flex-1 border rounded px-2 py-1 text-sm"
        />
      </div>
      <div className="flex-1 overflow-auto grid grid-cols-2 gap-2" data-testid="hf-grid">
        {status === "idle" && (
          <div className="col-span-2 text-xs text-gray-500 py-6 text-center" data-testid="hf-idle">
            Start typing to search Hugging Face.
          </div>
        )}
        {status === "loading" && (
          <div className="col-span-2 text-xs text-gray-500 py-6 text-center" data-testid="hf-loading">
            Searching…
          </div>
        )}
        {status === "error" && (
          <div className="col-span-2 text-xs text-red-600 py-6 text-center" role="alert" data-testid="hf-error-search">
            {error}
            <button type="button" onClick={() => setQuery(query + " ")} className="ml-2 underline">Retry</button>
          </div>
        )}
        {status === "ready" && hits.length === 0 && (
          <div className="col-span-2 text-xs text-gray-500 py-6 text-center" data-testid="hf-no-results">
            No models match. Try a different keyword.
          </div>
        )}
        {status === "ready" && hits.map((h) => (
          <button
            key={h.id}
            type="button"
            onClick={() => setSelected(h.id, h.tags)}
            data-testid={`hf-card-${h.id}`}
            className="border rounded p-3 text-left hover:bg-gray-50"
          >
            <div className="text-sm font-medium break-all">{h.id}</div>
            <div className="text-xs text-gray-500 mt-1">
              {h.downloads.toLocaleString()} downloads · {h.likes.toLocaleString()} likes
            </div>
            <div className="text-xs text-gray-400 mt-1">
              {h.tags.slice(0, 4).join(" · ")}
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
