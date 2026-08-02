import { useRef, useState } from "react";
import { PARAMS } from "./features/workspace/components/prompt/paramsInfo";
import { ParamRow } from "./features/workspace/components/prompt/ParamRow";
import { useParamsStore } from "./shared/state/paramsStore";
import { useBackendStore } from "./shared/state/backendStore";
import { useSelectedModelStore } from "./shared/state/selectedModelStore";
import { modelLabel } from "./shared/models/modelLabel";
import { useVramFit } from "./shared/memory/useVramFit";
import { usePopoverDismiss } from "./shared/ui/usePopoverDismiss";

/// The global inference-parameters popover in the header. Edits the single
/// globalParams every run reads. A "same for
/// all" toggle switches to per-model overrides. Reuses ParamRow + PARAMS so the
/// ranges/tooltips stay in one place.
export function ParamsControl() {
  const globalParams = useParamsStore((s) => s.globalParams);
  const setParam = useParamsStore((s) => s.setParam);
  const reset = useParamsStore((s) => s.reset);
  const sharedParams = useParamsStore((s) => s.sharedParams);
  const setSharedParams = useParamsStore((s) => s.setSharedParams);
  const perModelParams = useParamsStore((s) => s.perModelParams);
  const setModelParam = useParamsStore((s) => s.setModelParam);
  const selectedBackend = useBackendStore((s) => s.selectedBackend);
  const selectedModels = useSelectedModelStore((s) => s.selectedModels);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  usePopoverDismiss(open, ref, () => setOpen(false));

  const setCount = Object.values(globalParams).filter((v) => v !== undefined).length;
  // Per-model params needed a multi-model run, which no supported backend can serve.
  const perModelMode = false;
  // The model's max context window (num_ctx) — fetched only while the popover is
  // open, and only for a local model whose GGUF header can be read (the "Use max"
  // affordance below). A remote server exposes no dims, so it stays "Not available".
  const soloLocal = selectedBackend === "llama_cpp" && selectedModels.length === 1 ? selectedModels[0] : null;
  const { dims } = useVramFit(open && soloLocal ? soloLocal.name : undefined, soloLocal?.backend, globalParams.num_ctx ?? 4096);
  const modelMax = dims?.context_length ?? null;

  return (
    <div className="relative" data-testid="header-params" ref={ref}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="border rounded px-3 py-1 text-sm flex items-center gap-1"
        data-testid="header-params-button"
      >
        <span>Params</span>
        {setCount > 0 && <span className="text-blue-600" data-testid="header-params-count">· {setCount}</span>}
        <span className="text-gray-400">▾</span>
      </button>
      {open && (
        <div
          className="absolute right-0 z-20 mt-1 w-80 max-h-[28rem] overflow-y-auto bg-surface border rounded shadow p-3"
          data-testid="header-params-popover"
        >
          {perModelMode && (
            <label className="flex items-center gap-2 pb-2 text-xs text-gray-700" data-testid="header-shared-params">
              <input
                type="checkbox"
                checked={sharedParams}
                onChange={(e) => setSharedParams(e.target.checked)}
                data-testid="header-shared-params-toggle"
              />
              Use the same parameters for all models
            </label>
          )}

          {!perModelMode || sharedParams ? (
            PARAMS.map((info) => (
              <ParamRow
                key={info.key}
                info={info}
                value={globalParams[info.key]}
                onChange={(v) => setParam(info.key, v)}
              />
            ))
          ) : (
            selectedModels.map((m) => (
              <div key={m.name} className="border-t pt-1 mt-1" data-testid={`header-model-params-${m.name}`}>
                <div className="text-xs font-medium text-gray-700 truncate">{modelLabel(m)}</div>
                {PARAMS.map((info) => (
                  <ParamRow
                    key={info.key}
                    info={info}
                    value={perModelParams[m.name]?.[info.key] ?? globalParams[info.key]}
                    onChange={(v) => setModelParam(m.name, info.key, v)}
                  />
                ))}
              </div>
            ))
          )}

          {(!perModelMode || sharedParams) && modelMax != null && (
            <div className="flex items-center gap-2 border-t pt-2 mt-1 text-[11px] text-gray-600" data-testid="header-ctx-max">
              <span>Model max context: {modelMax.toLocaleString()}</span>
              <button
                type="button"
                onClick={() => setParam("num_ctx", modelMax)}
                className="text-blue-600 hover:text-blue-800 underline"
                data-testid="header-ctx-use-max"
              >
                Use max
              </button>
              <span className="text-gray-400">(larger = more memory)</span>
            </div>
          )}

          <div className="flex justify-end pt-1">
            <button
              type="button"
              onClick={reset}
              disabled={setCount === 0}
              className="text-xs text-gray-500 hover:text-gray-800 disabled:opacity-30"
              data-testid="header-params-reset"
            >
              Reset all
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
