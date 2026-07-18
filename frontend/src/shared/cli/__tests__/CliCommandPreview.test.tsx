import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { CliCommandPreview } from "../CliCommandPreview";

const show = vi.fn();
vi.mock("../../ui/Toast", () => ({ useToast: () => show }));

// jsdom here has no localStorage (--localstorage-file not set); back it with a Map.
const store = new Map<string, string>();
beforeEach(() => {
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
    },
  });
});

describe("CliCommandPreview", () => {
  beforeEach(() => {
    show.mockClear();
    localStorage.clear();
  });

  it("is collapsed by default — the command is hidden until expanded", () => {
    render(<CliCommandPreview cmd={{ command: "qm run --model m" }} />);
    expect(screen.getByTestId("cli-preview-toggle")).toBeInTheDocument();
    expect(screen.queryByTestId("cli-preview-command")).toBeNull();
  });

  it("expands to show the command, copies it, and toasts", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<CliCommandPreview cmd={{ command: "qm run --model qwen2.5:7b --k 8" }} />);
    fireEvent.click(screen.getByTestId("cli-preview-toggle"));
    expect(screen.getByTestId("cli-preview-command")).toHaveTextContent("qm run --model qwen2.5:7b --k 8");
    fireEvent.click(screen.getByTestId("cli-preview-copy"));
    await Promise.resolve();
    expect(writeText).toHaveBeenCalledWith("qm run --model qwen2.5:7b --k 8");
    expect(show).toHaveBeenCalledWith("Command copied");
  });

  it("persists the open state across mounts (localStorage)", () => {
    const { unmount } = render(<CliCommandPreview cmd={{ command: "qm run" }} />);
    fireEvent.click(screen.getByTestId("cli-preview-toggle")); // open
    unmount();
    render(<CliCommandPreview cmd={{ command: "qm run" }} />);
    expect(screen.getByTestId("cli-preview-command")).toBeInTheDocument(); // stays open
  });

  it("shows the note + a pick-a-model hint when incomplete", () => {
    localStorage.setItem("qm.cliPreview.open", "1");
    render(<CliCommandPreview cmd={{ command: "qm run --model <your-model>", incomplete: true, note: "do X first" }} />);
    const note = screen.getByTestId("cli-preview-note");
    expect(note).toHaveTextContent("Pick a model");
    expect(note).toHaveTextContent("do X first");
  });
});
