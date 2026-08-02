import { useModelInspect } from "../../hooks/useModelInspect";
import type { BackendKind } from "../../../../shared/ipc/models/storage";

/// Metadata inspector + template/base-model guard for an installed model. Shows
/// the raw chat template as inert text (never injected HTML), the reported
/// capabilities, and a base-model advisory when the metadata looks like a
/// text-completion model. Shows "Not available" when no backend exposes it.
export function TemplatePanel({ model, backend }: { model: string; backend: BackendKind }) {
  const { data, status } = useModelInspect(model, backend);

  if (status === "loading") return <p className="text-xs text-gray-500" data-testid="inspect-loading">Inspecting model…</p>;
  if (status === "error" || !data) return <p className="text-xs text-red-600" data-testid="inspect-error">Couldn't inspect this model.</p>;
  if (!data.available)
    return <p className="text-xs text-gray-500" data-testid="inspect-unavailable">{data.note ?? "Not available for this backend"}</p>;

  return (
    <div className="space-y-2 text-sm" data-testid="template-panel">
      {data.is_base_guess && (
        <p className="rounded border border-red-300 bg-red-50 px-2 py-1 text-xs text-red-700" data-testid="base-model-warning">
          ⚠ Likely a base model — it will ignore system prompts and tool-calling may be unreliable.
          {data.base_reason ? ` (${data.base_reason})` : ""}
        </p>
      )}
      <div className="flex flex-wrap gap-1.5" data-testid="inspect-capabilities">
        {data.capabilities.length === 0 ? (
          <span className="text-[11px] text-gray-400">no capabilities reported</span>
        ) : (
          data.capabilities.map((c) => (
            <span key={c} className="rounded bg-gray-100 px-2 py-0.5 text-[11px]">{c}</span>
          ))
        )}
      </div>
      <div>
        <span className="text-[11px] text-gray-400">Chat template</span>
        <pre className="mt-1 max-h-48 overflow-auto rounded border bg-gray-50 p-2 text-[11px]" data-testid="inspect-template">
          <code>{data.template || "(empty)"}</code>
        </pre>
      </div>
    </div>
  );
}
