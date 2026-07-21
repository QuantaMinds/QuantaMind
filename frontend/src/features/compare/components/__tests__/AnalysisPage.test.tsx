import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

// Isolate the container from the heavy tab body.
vi.mock("../AnalysisTab", () => ({
  AnalysisTab: () => <div data-testid="stub-analysis-body" />,
}));

import { AnalysisPage } from "../AnalysisPage";

describe("AnalysisPage", () => {
  it("renders the Analysis body directly — the Quant sub-tab is gone with its feature", () => {
    render(<AnalysisPage />);
    expect(screen.getByTestId("stub-analysis-body")).toBeInTheDocument();
    expect(screen.queryByTestId("analysis-tablist")).toBeNull();
    expect(screen.queryByTestId("analysis-tab-quant")).toBeNull();
  });
});
