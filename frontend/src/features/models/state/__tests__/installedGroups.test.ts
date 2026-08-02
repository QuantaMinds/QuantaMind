import { describe, it, expect } from "vitest";
import { groupInstalled } from "../installedGroups";

const m = (name: string, backend: "llama_cpp" | "vllm", path?: string) => ({
  name, size_bytes: 1, modified_at: "", family: "x", parameter_size: "1B",
  quantization: "Q4", backend, path,
});

describe("groupInstalled", () => {
  it("collapses the same name served locally and remotely into one row", () => {
    const groups = groupInstalled([m("phi-4-mini", "vllm"), m("phi-4-mini", "llama_cpp", "/g/phi-4-mini.gguf")]);
    expect(groups).toHaveLength(1);
    expect(groups[0]).toMatchObject({ name: "phi-4-mini", llamaPath: "/g/phi-4-mini.gguf" });
  });

  it("carries the GGUF path for a locally-installed model (the delete target)", () => {
    const [g] = groupInstalled([m("solo", "llama_cpp", "/g/solo.gguf")]);
    expect(g.llamaPath).toBe("/g/solo.gguf");
  });

  it("leaves a remote-only model with no local path, so it offers no delete", () => {
    const [g] = groupInstalled([m("served-elsewhere", "vllm")]);
    expect(g.llamaPath).toBeUndefined();
  });

  it("keeps distinct model names apart and sorts them", () => {
    const groups = groupInstalled([m("mistral:7b", "llama_cpp"), m("llama3:8b", "llama_cpp")]);
    expect(groups.map((g) => g.name)).toEqual(["llama3:8b", "mistral:7b"]);
  });

  it("prefers a friendly display name when the wire name isn't presentable", () => {
    const [g] = groupInstalled([{
      name: "/m/repo_X", size_bytes: 1, modified_at: "", family: "x",
      parameter_size: "", quantization: "Q4", backend: "llama_cpp" as const,
      path: "/m/repo_X", display_name: "repo/X",
    }]);
    expect(g.displayName).toBe("repo/X");
  });
});
