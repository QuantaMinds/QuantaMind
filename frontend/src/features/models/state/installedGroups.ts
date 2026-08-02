import type { InstalledModelInfo } from "../../../shared/ipc/models/storage";

export interface ModelGroup {
  name: string;
  family: string;
  parameterSize: string;
  quantization: string;
  sizeBytes: number;
  llamaPath?: string; // folder GGUF path (for delete), present if installed locally
  displayName?: string; // friendly label when `name` isn't presentable
}

/// Collapse the merged installed list into one entry per model so the UI shows
/// no duplicate rows when the same name is served both locally and remotely.
export function groupInstalled(list: InstalledModelInfo[]): ModelGroup[] {
  const map = new Map<string, ModelGroup>();
  for (const m of list) {
    const g = map.get(m.name) ?? {
      name: m.name,
      family: m.family,
      parameterSize: m.parameter_size,
      quantization: m.quantization,
      sizeBytes: m.size_bytes,
    };
    if (m.backend === "llama_cpp") g.llamaPath = m.path ?? g.llamaPath;
    g.displayName = m.display_name ?? g.displayName;
    map.set(m.name, g);
  }
  return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
}
