import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@monaco-editor/react", () => ({
  default: () => <textarea data-testid="prompt-input" />,
}));

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import App from "../App";

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockResolvedValue(() => {});
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "list_models") return Promise.resolve([]);
    if (cmd === "check_llama_health")
      return Promise.resolve({ available: true, version: "x" });
    return Promise.reject(new Error(`unknown ${cmd}`));
  });
});

describe("App after Workspace extraction", () => {
  it("renders the QuantaMind heading and the model selector", async () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "QuantaMind" })).toBeTruthy();
    expect(await screen.findByTestId("header-model-select")).toBeTruthy();
  });
});
