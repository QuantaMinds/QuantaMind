import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { McpTaskBuilder } from "../components/McpTaskBuilder";
import type { CollectionValidation } from "../../../shared/ipc/eval/registry";

// The store: capture addTask so we can assert a broken task is NOT added.
const addTask = vi.fn();
vi.mock("../state/mcpStore", () => ({
  useMcpStore: (sel: (s: { addTask: typeof addTask }) => unknown) => sel({ addTask }),
}));

// The validation IPC — driven per test.
const validateMcpTasks = vi.fn();
vi.mock("../../../shared/ipc/mcp/run", () => ({ validateMcpTasks: (...a: unknown[]) => validateMcpTasks(...a) }));

const WORLD_JSON = JSON.stringify({
  name: "summarize",
  instruction: "Read notes.txt, write summary.md",
  world: { type: "fs", files: [{ path: "notes.txt", content: "alpha" }] },
  oracle: { assert_present: ["summary.md"] },
});

function paste(text: string) {
  fireEvent.change(screen.getByPlaceholderText(/"name":/), { target: { value: text } });
  fireEvent.click(screen.getByText("Add from JSON"));
}

describe("McpTaskBuilder — auto-validate on JSON import", () => {
  beforeEach(() => {
    addTask.mockClear();
    validateMcpTasks.mockReset();
  });

  it("BLOCKS a broken oracle: shows the finding and never adds the task", async () => {
    const verdict: CollectionValidation = {
      ok: false,
      structural_error: null,
      tasks: [{
        id: "mcp-summarize-0",
        reachable: "yes",
        discriminating: false,
        detail: "",
        semantic: ["world: the untouched seed already satisfies the oracle — a do-nothing agent passes"],
        semantic_warnings: [],
      }],
    };
    validateMcpTasks.mockResolvedValue(verdict);
    render(<McpTaskBuilder />);
    paste(WORLD_JSON);

    await waitFor(() => expect(screen.getByTestId("mcp-validation-result")).toBeInTheDocument());
    expect(screen.getByTestId("mcp-validation-result")).toHaveTextContent("Not added");
    expect(screen.getByTestId("mcp-validation-semantic-mcp-summarize-0-0")).toHaveTextContent("do-nothing agent passes");
    expect(addTask).not.toHaveBeenCalled(); // the broken world never enters the builder
  });

  it("ADDS a valid world and reports it", async () => {
    validateMcpTasks.mockResolvedValue({ ok: true, structural_error: null, tasks: [{ id: "mcp-summarize-0", reachable: "yes", discriminating: true, detail: "", semantic: [], semantic_warnings: [] }] });
    render(<McpTaskBuilder />);
    paste(WORLD_JSON);

    await waitFor(() => expect(addTask).toHaveBeenCalledTimes(1));
    // A clean add closes on success — no error, no blocking panel.
    expect(screen.queryByTestId("mcp-validation-result")).toBeNull();
  });

  it("shows a readable message for non-JSON without calling validation", () => {
    render(<McpTaskBuilder />);
    paste("{ not json");
    expect(screen.getByTestId("mcp-json-error")).toBeInTheDocument();
    expect(validateMcpTasks).not.toHaveBeenCalled();
    expect(addTask).not.toHaveBeenCalled();
  });
});
