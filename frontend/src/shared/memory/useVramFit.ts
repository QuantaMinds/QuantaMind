import { useEffect, useState } from "react";
import { inspectModel, estimateKvCacheBytes, type ModelDims } from "../ipc/system/inspect";
import type { BackendKind } from "../ipc/models/storage";

/// Fetch a model's architecture dims (Ollama /api/show) and the KV-cache bytes
/// for the chosen context length. `dims`/`kvBytes` are null when unavailable
/// (non-Ollama, or metadata missing) — the caller then falls back to the
/// file-size heuristic. The KV math is the canonical Rust formula, not a copy.
/// `capabilities` rides along from the same /api/show response (null = not
/// reported / probe failed / non-Ollama) — Ollama grants `tools` only when the
/// model's template references `.Tools`, so callers gate native FC on it.
export function useVramFit(model: string | undefined, backend: BackendKind | undefined, ctxLen: number) {
  const [dims, setDims] = useState<ModelDims | null>(null);
  const [capabilities, setCapabilities] = useState<string[] | null>(null);
  const [kvBytes, setKvBytes] = useState<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!model || !backend) {
      setDims(null);
      setCapabilities(null);
      return;
    }
    setCapabilities(null);
    inspectModel(model, backend)
      .then((r) => {
        if (cancelled) return;
        setDims(r.dims);
        setCapabilities(r.available ? r.capabilities : null);
      })
      .catch(() => {
        if (cancelled) return;
        setDims(null);
        setCapabilities(null);
      });
    return () => {
      cancelled = true;
    };
  }, [model, backend]);

  useEffect(() => {
    let cancelled = false;
    if (!dims) {
      setKvBytes(null);
      return;
    }
    estimateKvCacheBytes(dims, ctxLen)
      .then((b) => !cancelled && setKvBytes(b))
      .catch(() => !cancelled && setKvBytes(null));
    return () => {
      cancelled = true;
    };
  }, [dims, ctxLen]);

  return { dims, kvBytes, capabilities };
}
