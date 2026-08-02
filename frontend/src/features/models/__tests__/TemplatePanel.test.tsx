import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../../shared/ipc/system/inspect", () => ({ inspectModel: vi.fn() }));

import { inspectModel } from "../../../shared/ipc/system/inspect";
import { TemplatePanel } from "../components/card/TemplatePanel";

const base = {
  available: true, note: null, template: "{{ .Prompt }}", capabilities: ["completion"],
  family: "llama", parameter_size: "7B", quantization: "Q4_K_M",
  is_base_guess: false, base_reason: null, dims: null,
};

beforeEach(() => vi.clearAllMocks());

describe("TemplatePanel", () => {
  it("shows the template + capabilities for an instruct model, no warning", async () => {
    vi.mocked(inspectModel).mockResolvedValue({ ...base, capabilities: ["completion", "tools"], template: "<|assistant|>" });
    render(<TemplatePanel model="m" backend="llama_cpp" />);
    await waitFor(() => expect(screen.getByTestId("inspect-template")).toHaveTextContent("<|assistant|>"));
    expect(screen.getByTestId("inspect-capabilities")).toHaveTextContent("tools");
    expect(screen.queryByTestId("base-model-warning")).toBeNull();
  });

  it("renders the base-model advisory with its reason", async () => {
    vi.mocked(inspectModel).mockResolvedValue({ ...base, is_base_guess: true, base_reason: "no 'tools' capability" });
    render(<TemplatePanel model="m" backend="llama_cpp" />);
    await waitFor(() => expect(screen.getByTestId("base-model-warning")).toHaveTextContent("Likely a base model"));
    expect(screen.getByTestId("base-model-warning")).toHaveTextContent("no 'tools' capability");
  });

  it("shows 'Not available' when no backend exposes the template", async () => {
    vi.mocked(inspectModel).mockResolvedValue({ ...base, available: false, note: "Not available for this backend" });
    render(<TemplatePanel model="m" backend="vllm" />);
    await waitFor(() => expect(screen.getByTestId("inspect-unavailable")).toHaveTextContent("Not available for this backend"));
  });
});
