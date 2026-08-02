import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

import { DownloadsPage } from "../DownloadsPage";
import { useModelStore } from "../../state/modelStore";

beforeEach(() => {
  useModelStore.setState({ downloads: {}, activeHfName: null });
});

describe("DownloadsPage", () => {
  it("renders the Downloads heading and embeds the DownloadsTab body", () => {
    render(<DownloadsPage />);
    expect(screen.getByRole("heading", { name: /Downloads/ })).toBeInTheDocument();
    expect(screen.getByTestId("page-downloads")).toBeInTheDocument();
    expect(screen.getByTestId("downloads-tab")).toBeInTheDocument();
  });
});
