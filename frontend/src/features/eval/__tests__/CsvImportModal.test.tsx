import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { CsvImportModal } from "../components/manager/CsvImportModal";

// The file picker isn't exercised in the paste-driven path; stub the Tauri dialog
// plugin so importing the modal doesn't blow up in jsdom.
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const HEADER = "id,prompt,expected_tool,expected_args";
const VALID_CSV = `${HEADER}\nweather-paris,"Weather in Paris?",get_weather,"{""city"":""Paris""}"\nrefuse-greet,"Say hello",,`;

beforeEach(() => vi.clearAllMocks());

describe("CsvImportModal", () => {
  it("renders the format guide example table", () => {
    render(<CsvImportModal onImport={vi.fn()} onClose={vi.fn()} />);
    expect(screen.getByTestId("csv-import-example")).toBeInTheDocument();
    expect(screen.getByTestId("csv-import-modal")).toBeInTheDocument();
  });

  it("shows no validation error before any CSV is entered", () => {
    render(<CsvImportModal onImport={vi.fn()} onClose={vi.fn()} />);
    // The untouched modal must not scream "CSV is empty" — the panel is gated on input.
    expect(screen.queryByTestId("csv-import-preview")).toBeNull();
    expect(screen.queryByTestId("csv-import-header-error")).toBeNull();
    expect(screen.getByTestId("csv-import-submit")).toBeDisabled();
  });

  it("previews valid rows, enables import, and calls onImport with mapped tasks", async () => {
    const onImport = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    render(<CsvImportModal onImport={onImport} onClose={onClose} />);

    fireEvent.change(screen.getByTestId("csv-import-paste"), { target: { value: VALID_CSV } });
    fireEvent.change(screen.getByTestId("csv-import-name"), { target: { value: "my-cases" } });

    expect(screen.getByTestId("csv-row-ok-1")).toBeInTheDocument();
    expect(screen.getByTestId("csv-row-ok-2")).toBeInTheDocument();

    const submit = screen.getByTestId("csv-import-submit");
    expect(submit).not.toBeDisabled();
    expect(submit).toHaveTextContent("Import 2 tasks");

    fireEvent.click(submit);
    await waitFor(() => expect(onImport).toHaveBeenCalledTimes(1));
    const [name, tasks] = onImport.mock.calls[0];
    expect(name).toBe("my-cases");
    expect(tasks).toHaveLength(2);
    expect(tasks[1]).toMatchObject({ category: "abstain", expected: { type: "no_call" } });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("shows a located row error and keeps import disabled for a bad row", () => {
    render(<CsvImportModal onImport={vi.fn()} onClose={vi.fn()} />);
    fireEvent.change(screen.getByTestId("csv-import-paste"), {
      target: { value: `${HEADER}\nx,"p",get_weather,"{bad json}"` },
    });
    fireEvent.change(screen.getByTestId("csv-import-name"), { target: { value: "x" } });
    expect(screen.getByTestId("csv-row-err-1")).toHaveTextContent("not valid JSON");
    expect(screen.getByTestId("csv-import-submit")).toBeDisabled();
  });

  it("flags a misordered header and blocks import", () => {
    render(<CsvImportModal onImport={vi.fn()} onClose={vi.fn()} />);
    fireEvent.change(screen.getByTestId("csv-import-paste"), {
      target: { value: "id,prompt,tool,expected_args\nx,p,get_weather,{}" },
    });
    expect(screen.getByTestId("csv-import-header-error")).toHaveTextContent("expected_tool");
    expect(screen.getByTestId("csv-import-submit")).toBeDisabled();
  });

  it("Learn more navigates to the Docs reference (CSV-import block)", async () => {
    const { useNavStore } = await import("../../../shared/state/navStore");
    render(<CsvImportModal onImport={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByTestId("csv-import-learnmore"));
    expect(useNavStore.getState().topView).toBe("docs");
    await waitFor(() => expect(location.hash).toBe("#docs-reference-eval--collection-editor-csv-import"));
  });
});
