import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("../../../../shared/ipc/settings/userSettings", () => ({
  resolveModelsFolder: vi.fn(),
  getUserSettings: vi.fn(),
  setUserSettings: vi.fn(),
}));

import { open } from "@tauri-apps/plugin-dialog";
import {
  resolveModelsFolder,
  getUserSettings,
  setUserSettings,
} from "../../../../shared/ipc/settings/userSettings";
import { ModelsFolderSection } from "../ModelsFolderSection";

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(resolveModelsFolder).mockResolvedValue("/home/u/.quantamind/gguf");
  vi.mocked(getUserSettings).mockResolvedValue({ first_run_complete: true });
  vi.mocked(setUserSettings).mockResolvedValue(undefined);
});

describe("ModelsFolderSection", () => {
  it("shows the resolved weights folder", async () => {
    render(<ModelsFolderSection />);
    await waitFor(() =>
      expect(screen.getByTestId("models-folder-path"))
        .toHaveTextContent("/home/u/.quantamind/gguf"),
    );
  });

  it("Change… picks a folder and persists it", async () => {
    vi.mocked(open).mockResolvedValue("/models/shared");
    render(<ModelsFolderSection />);
    await screen.findByTestId("models-folder-path");
    fireEvent.click(screen.getByTestId("models-folder-change"));
    await waitFor(() =>
      expect(setUserSettings).toHaveBeenCalledWith(
        expect.objectContaining({ models_folder: "/models/shared" }),
      ),
    );
  });

  it("does nothing when the picker is cancelled", async () => {
    vi.mocked(open).mockResolvedValue(null);
    render(<ModelsFolderSection />);
    await screen.findByTestId("models-folder-path");
    fireEvent.click(screen.getByTestId("models-folder-change"));
    await waitFor(() => expect(open).toHaveBeenCalled());
    expect(setUserSettings).not.toHaveBeenCalled();
  });
});
