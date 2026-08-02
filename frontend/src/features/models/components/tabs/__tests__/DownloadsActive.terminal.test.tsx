import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../../../../shared/ipc/models/hf_install", () => ({
  cancelHfInstall: vi.fn(),
}));

import { DownloadsActive } from "../DownloadsActive";
import { useModelStore } from "../../../state/modelStore";

beforeEach(() => {
  useModelStore.setState({ downloads: {} });
});

describe("DownloadsActive — terminal states", () => {
  it("success entries render with Installed ✓ label and a Dismiss button", () => {
    useModelStore.getState().upsertDownload({
      id: "done", source: "huggingface", name: "done",
      status: "success", percent: 100,
    });
    render(<DownloadsActive />);
    expect(screen.getByTestId("download-active-done")).toHaveTextContent("Installed ✓");
    expect(screen.getByRole("button", { name: /dismiss done/i })).toBeInTheDocument();
  });

  it("error entries surface the error message and a Dismiss button", () => {
    useModelStore.getState().upsertDownload({
      id: "broke", source: "huggingface", name: "broke",
      status: "error", percent: 0, error: "registration rolled back",
    });
    render(<DownloadsActive />);
    expect(screen.getByTestId("download-error-broke"))
      .toHaveTextContent("registration rolled back");
    expect(screen.getByRole("button", { name: /dismiss broke/i })).toBeInTheDocument();
  });

  it("Dismiss removes the entry from the store", () => {
    useModelStore.getState().upsertDownload({
      id: "done", source: "huggingface", name: "done",
      status: "success", percent: 100,
    });
    render(<DownloadsActive />);
    fireEvent.click(screen.getByRole("button", { name: /dismiss done/i }));
    expect(useModelStore.getState().downloads["done"]).toBeUndefined();
  });
});
