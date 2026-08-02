import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MatrixGrid } from "../../components/matrix/MatrixGrid";

const tasks = [{
  id: "w", category: "single", prompt: "p",
  tools: [{ name: "x", description: "", parameters: { type: "object", properties: {} } }],
  expected: { type: "call", name: "x", args: {} },
}] as never;

const report = {
  collection_id: "c",
  avg_score: 0.5,
  columns: [
    {
      model: "m1", backend: "llama_cpp", error: null,
      report: { n: 1, parse_rate: 1, tool_selection_acc: 1, arg_acc: 0, abstain_acc: null, composite: 0.66,
        per_task: [{ id: "w", category: "single", verdict: { parsed: true, tool_match: true, args_match: false, abstain_correct: null } }] },
    },
    { model: "m2", backend: "llama_cpp", report: null, error: "server down" },
  ],
} as never;

describe("MatrixGrid", () => {
  it("prompts to run when there is no report", () => {
    render(<MatrixGrid tasks={tasks} report={null} />);
    expect(screen.getByTestId("eval-matrix-empty")).toBeTruthy();
  });

  it("renders P/T/A per task×model and dashes for a failed column", () => {
    render(<MatrixGrid tasks={tasks} report={report} />);
    const ok = screen.getByTestId("eval-matrix-cell-w-m1");
    expect(ok).toHaveTextContent("P");
    expect(ok).toHaveTextContent("A");
    expect(screen.getByTestId("eval-matrix-cell-w-m2")).toHaveTextContent("—");
  });

  it("hands (collection, task, model) to onViewTrace when a scored cell is clicked", () => {
    const onViewTrace = vi.fn();
    render(<MatrixGrid tasks={tasks} report={report} onViewTrace={onViewTrace} />);

    fireEvent.click(screen.getByTestId("eval-matrix-cell-w-m1").querySelector("button")!);
    expect(onViewTrace).toHaveBeenCalledWith({ collection: "c", taskId: "w", model: "m1" });

    // A not-run cell ("—") is inert — no button to click.
    expect(screen.getByTestId("eval-matrix-cell-w-m2").querySelector("button")).toBeNull();
  });
});
