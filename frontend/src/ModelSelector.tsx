import { useEffect, useRef, useState } from "react";
import { useInstalledModelsStore } from "./features/models/state/installedModelsStore";
import { isEmbeddingModel, isLikelyThinkingModel } from "./shared/models/classify";
import { dedupeByDigest } from "./shared/models/dedupeDigest";
import { modelLabel } from "./shared/models/modelLabel";
import { formatBytes } from "./shared/format/bytes";
import { useBackendStore } from "./shared/state/backendStore";
import { useSelectedModelStore } from "./shared/state/selectedModelStore";
import { useParamsStore } from "./shared/state/paramsStore";
import { useModelSettingsStore } from "./features/models/state/modelSettingsStore";
import { usePopoverDismiss } from "./shared/ui/usePopoverDismiss";

/// The global model picker in the header, filtered to the selected backend.
/// Single-select: every supported backend serves one model per launch, so there
/// is no multi-model selection. Writes the global selectedModelStore — every page
/// reads it.
export function ModelSelector() {
  const selectedBackend = useBackendStore((s) => s.selectedBackend);
  const selectedModels = useSelectedModelStore((s) => s.selectedModels);
  const setSelectedModels = useSelectedModelStore((s) => s.setSelectedModels);
  const list = useInstalledModelsStore((s) => s.list);
  const status = useInstalledModelsStore((s) => s.status);
  const error = useInstalledModelsStore((s) => s.error);
  const refresh = useInstalledModelsStore((s) => s.refresh);
  const keepLoaded = useParamsStore((s) => s.keepLoaded);
  const setKeepLoaded = useParamsStore((s) => s.setKeepLoaded);
  const byModel = useModelSettingsStore((s) => s.byModel);
  const setThinking = useModelSettingsStore((s) => s.setThinking);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  const generative = dedupeByDigest(
    list.filter((m) => !isEmbeddingModel(m) && m.backend === selectedBackend),
  );
  const has = (name: string) => selectedModels.some((s) => s.name === name);

  useEffect(() => { if (status === "idle") void refresh(); }, [status, refresh]);
  usePopoverDismiss(open, ref, () => setOpen(false));

  const pick = (m: { name: string; size_bytes: number; backend: typeof selectedBackend; path?: string }) => {
    const entry = { name: m.name, backend: m.backend, size_bytes: m.size_bytes, path: m.path };
    setSelectedModels(has(m.name) ? [] : [entry]);
    setOpen(false);
  };

  const labelFor = (name: string) => modelLabel(list.find((x) => x.name === name) ?? { name });
  const label = selectedModels.length === 0 ? "Select a model"
    : selectedModels.length === 1 ? labelFor(selectedModels[0].name)
    : `${selectedModels.length} models`;

  return (
    <div className="relative" data-testid="header-model-select" ref={ref}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="border rounded px-3 py-1 text-sm min-w-[12rem] text-left flex justify-between gap-2"
        data-testid="header-model-dropdown"
      >
        <span className="truncate">{label}</span>
        <span className="text-gray-400">▾</span>
      </button>
      {open && (
        <div className="absolute z-20 mt-1 w-72 max-h-72 overflow-y-auto bg-surface border rounded shadow text-sm">
          <label
            className="flex items-center gap-2 px-3 py-2 border-b text-xs text-gray-600 select-none"
            title="Keep the model resident across runs. Off lets it unload when idle, freeing memory; the just-run model stays inspectable for a few minutes."
          >
            <input
              type="checkbox"
              checked={keepLoaded}
              onChange={(e) => setKeepLoaded(e.target.checked)}
              data-testid="header-keep-loaded"
            />
            Keep model loaded
          </label>
          {(status === "idle" || status === "loading") && <div className="px-3 py-2 text-gray-500">Loading…</div>}
          {status === "error" && (
            <div role="alert" className="px-3 py-2 text-red-600">
              {error}{" "}
              <button type="button" onClick={() => void refresh()} className="underline">Retry</button>
            </div>
          )}
          {status === "ready" && generative.length === 0 && (
            <div className="px-3 py-2 text-gray-500">No models for this backend.</div>
          )}
          {generative.map((m) => {
            const active = has(m.name);
            // Explicit per-model setting wins; otherwise auto-detect from the name so a
            // reasoning model is pre-checked and the user doesn't have to guess.
            const thinking = byModel[m.name]?.is_thinking ?? isLikelyThinkingModel(m.name);
            return (
              <button
                key={m.name}
                type="button"
                onClick={() => pick(m)}
                data-testid={`header-model-option-${m.name}`}
                aria-pressed={active}
                className={`w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-gray-100 ${active ? "bg-blue-50" : ""}`}
              >
                <span className="flex-1 truncate">{modelLabel(m)}</span>
                <span
                  role="checkbox"
                  aria-checked={thinking}
                  aria-label="Thinking model"
                  title="Reasoning model: allow a larger token budget for its <think> step"
                  tabIndex={0}
                  data-testid={`header-model-thinking-${m.name}`}
                  onClick={(e) => { e.stopPropagation(); void setThinking(m.name, !thinking); }}
                  onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.stopPropagation(); void setThinking(m.name, !thinking); } }}
                  className={`flex shrink-0 cursor-pointer items-center gap-1 text-[10px] leading-none ${thinking ? "text-purple-700" : "text-gray-500"}`}
                >
                  <span className={`inline-flex h-3.5 w-3.5 items-center justify-center rounded border text-[9px] leading-none ${thinking ? "bg-purple-600 border-purple-600 text-white" : "border-gray-400"}`}>
                    {thinking ? "✓" : ""}
                  </span>
                  Thinking
                </span>
                {m.size_bytes > 0 && (
                  <span className="text-[10px] text-gray-400">{formatBytes(m.size_bytes)}</span>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
