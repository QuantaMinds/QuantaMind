import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-shell", () => ({
  open: vi.fn().mockResolvedValue(undefined),
}));

import { open as openExternal } from "@tauri-apps/plugin-shell";
import { FeedbackModal } from "../components/FeedbackModal";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";

const onClose = vi.fn();

beforeEach(() => {
  vi.mocked(openExternal).mockReset().mockResolvedValue(undefined);
  onClose.mockReset();
  useSelectedModelStore.setState({ selectedModels: [] });
});

const renderModal = () => render(<FeedbackModal onClose={onClose} />);

describe("FeedbackModal (mailto flow)", () => {
  it("Send is disabled until message is at least 10 chars (trimmed)", () => {
    renderModal();
    const send = screen.getByTestId("feedback-send");
    expect(send).toBeDisabled();
    fireEvent.change(screen.getByTestId("feedback-message"), {
      target: { value: "   short   " },
    });
    expect(send).toBeDisabled();
    fireEvent.change(screen.getByTestId("feedback-message"), {
      target: { value: "this is a real complaint" },
    });
    expect(send).toBeEnabled();
  });

  it("opens a mailto: URL with subject + body and closes the modal", async () => {
    renderModal();
    fireEvent.change(screen.getByTestId("feedback-message"), {
      target: { value: "the search is sluggish on big repos" },
    });
    fireEvent.click(screen.getByTestId("feedback-send"));
    await waitFor(() => expect(openExternal).toHaveBeenCalledTimes(1));
    const url = vi.mocked(openExternal).mock.calls[0][0] as string;
    expect(url.startsWith("mailto:info@quantamind.co?")).toBe(true);
    expect(url).toContain("subject=QuantaMind+Feedback");
    expect(decodeURIComponent(url.replace(/\+/g, " "))).toContain("the search is sluggish on big repos");
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("includes diagnostics + current model in the body when checkbox is on", async () => {
    useSelectedModelStore.setState({ selectedModels: [{ name: "mistral:7b", backend: "llama_cpp", size_bytes: 1 }] });
    renderModal();
    fireEvent.change(screen.getByTestId("feedback-message"), {
      target: { value: "ten chars min satisfied" },
    });
    fireEvent.click(screen.getByTestId("feedback-diagnostics"));
    fireEvent.click(screen.getByTestId("feedback-send"));
    await waitFor(() => expect(openExternal).toHaveBeenCalledTimes(1));
    const url = vi.mocked(openExternal).mock.calls[0][0] as string;
    const body = decodeURIComponent(url.replace(/\+/g, " "));
    expect(body).toContain("Diagnostics (opt-in)");
    expect(body).toContain("Model: mistral:7b");
    expect(body).toContain("App: QuantaMind v");
  });

  it("omits diagnostics block when checkbox is off", async () => {
    renderModal();
    fireEvent.change(screen.getByTestId("feedback-message"), {
      target: { value: "ten chars min satisfied" },
    });
    fireEvent.click(screen.getByTestId("feedback-send"));
    await waitFor(() => expect(openExternal).toHaveBeenCalledTimes(1));
    const body = decodeURIComponent((vi.mocked(openExternal).mock.calls[0][0] as string).replace(/\+/g, " "));
    expect(body).not.toContain("Diagnostics");
    expect(body).not.toContain("Model:");
  });

  it("surfaces a shell.open rejection inline and keeps the modal open", async () => {
    vi.mocked(openExternal).mockRejectedValue(new Error("no default mail client"));
    renderModal();
    fireEvent.change(screen.getByTestId("feedback-message"), {
      target: { value: "ten chars min satisfied" },
    });
    fireEvent.click(screen.getByTestId("feedback-send"));
    await waitFor(() =>
      expect(screen.getByTestId("feedback-error")).toHaveTextContent(/no default mail client/),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("Cancel closes without launching the mail app", () => {
    renderModal();
    fireEvent.click(screen.getByTestId("feedback-cancel"));
    expect(onClose).toHaveBeenCalled();
    expect(openExternal).not.toHaveBeenCalled();
  });

  it("Escape closes the modal", () => {
    renderModal();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
