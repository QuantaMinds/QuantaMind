import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

import { ModelsPage } from "../ModelsPage";
import { useModelStore } from "../../state/modelStore";
import { useNavStore } from "../../../../shared/state/navStore";

beforeEach(() => {
  useModelStore.setState({
    activeTab: "huggingface",
    downloads: {},
    activeHfName: null,
    hfSearchQuery: "",
    hfSelectedRepo: null,
  });
  useNavStore.setState({ topView: "models" });
});

describe("ModelsPage", () => {
  it("renders both sub-tab buttons in order", () => {
    render(<ModelsPage />);
    expect(screen.getByTestId("models-tab-huggingface")).toHaveTextContent("Hugging Face");
    expect(screen.getByTestId("models-tab-local")).toHaveTextContent("Local File");
  });

  it("Hugging Face is the active sub-tab by default", () => {
    render(<ModelsPage />);
    expect(screen.getByTestId("models-tab-huggingface")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("models-tab-local")).toHaveAttribute("aria-selected", "false");
  });

  it("clicking a sub-tab updates modelStore.activeTab and renders that tab's body", () => {
    render(<ModelsPage />);
    fireEvent.click(screen.getByTestId("models-tab-huggingface"));
    expect(useModelStore.getState().activeTab).toBe("huggingface");
    expect(screen.getByTestId("tab-huggingface")).toBeInTheDocument();
  });

  it("Cmd+1/2 switch sub-tabs when topView is 'models'", () => {
    render(<ModelsPage />);
    fireEvent.keyDown(document, { key: "2", metaKey: true });
    expect(useModelStore.getState().activeTab).toBe("local");
    fireEvent.keyDown(document, { key: "1", metaKey: true });
    expect(useModelStore.getState().activeTab).toBe("huggingface");
  });

  it("Cmd+1/2 are no-ops when topView is NOT 'models'", () => {
    useNavStore.setState({ topView: "workspace" });
    render(<ModelsPage />);
    fireEvent.keyDown(document, { key: "2", metaKey: true });
    expect(useModelStore.getState().activeTab).toBe("huggingface");
  });
});
