import { useEffect, useState } from "react";
import { inspectModel, contextCeilings, type ModelDims, type CtxCeilings } from "../../../shared/ipc/system/inspect";
import type { BackendKind } from "../../../shared/ipc/models/storage";

/// The context-ceiling meters' data: how much context this machine holds for the
/// model at f16 / q8_0 / q4_0 KV precision, plus the model's own declared max
/// (for the "model max" tick and clamp). `dims`/`ceilings` are null when
/// unmeasurable (non-Ollama, or metadata missing) — the component then renders
/// "Not available", never a fabricated ceiling. The ceiling math is the same Rust
/// the launch planner uses (single source of truth), fetched via IPC.
export function useKvCeilings(
  model: string | undefined,
  backend: BackendKind | undefined,
  modelBytes: number | null | undefined,
  totalBytes: number | null | undefined,
) {
  const [dims, setDims] = useState<ModelDims | null>(null);
  const [ceilings, setCeilings] = useState<CtxCeilings | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!model || !backend) {
      setDims(null);
      return;
    }
    inspectModel(model, backend)
      .then((r) => !cancelled && setDims(r.dims))
      .catch(() => !cancelled && setDims(null));
    return () => {
      cancelled = true;
    };
  }, [model, backend]);

  useEffect(() => {
    let cancelled = false;
    if (!dims || !modelBytes || !totalBytes) {
      setCeilings(null);
      return;
    }
    contextCeilings(dims, modelBytes, totalBytes)
      .then((c) => !cancelled && setCeilings(c))
      .catch(() => !cancelled && setCeilings(null));
    return () => {
      cancelled = true;
    };
  }, [dims, modelBytes, totalBytes]);

  return { dims, ceilings };
}
