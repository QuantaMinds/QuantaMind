import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";

vi.mock("../../../../shared/ipc/settings/userSettings", () => ({
  getUserSettings: vi.fn(),
  setUserSettings: vi.fn(),
}));
vi.mock("../../../models/state/installedModelsStore", () => ({
  useInstalledModelsStore: { getState: () => ({ refresh: vi.fn().mockResolvedValue(undefined) }) },
}));

import { RemoteBackendsSection } from "../RemoteBackendsSection";
import { getUserSettings, setUserSettings } from "../../../../shared/ipc/settings/userSettings";

beforeEach(() => vi.clearAllMocks());

describe("RemoteBackendsSection", () => {
  it("surfaces the backend's https-only guardrail message instead of a bare failure", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({
      vllm_url: "http://34.10.20.30:8000",
      vllm_api_key: "sk-secret",
    } as never);
    // The backend rejects with a serialized AppError::Validation.
    vi.mocked(setUserSettings).mockRejectedValue({
      kind: "validation",
      message:
        "vLLM: refusing to store the API key — http://34.10.20.30:8000 is not HTTPS, so the key would be sent in cleartext.",
    });

    render(<RemoteBackendsSection />);
    await waitFor(() => expect(screen.getByTestId("remote-backends-save")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("remote-backends-save"));

    const err = await screen.findByTestId("remote-backends-error");
    expect(err.textContent).toContain("not HTTPS");
    expect(err.textContent).toContain("cleartext");
  });

  it("shows a Saved confirmation on success", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({ vllm_url: "https://host:8000" } as never);
    vi.mocked(setUserSettings).mockResolvedValue(undefined as never);

    render(<RemoteBackendsSection />);
    await waitFor(() => expect(screen.getByTestId("remote-backends-save")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("remote-backends-save"));

    await waitFor(() => expect(screen.getByText("Saved")).toBeInTheDocument());
    expect(screen.queryByTestId("remote-backends-error")).not.toBeInTheDocument();
  });
});
