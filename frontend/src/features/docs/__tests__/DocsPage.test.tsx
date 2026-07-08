import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import { DocsPage } from "../components/DocsPage";
import { useNavStore } from "../../../shared/state/navStore";

vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));

beforeEach(() => {
  useNavStore.setState({ topView: "docs" });
  window.location.hash = "";
});

describe("DocsPage", () => {
  it("shows the default Getting started page", () => {
    render(<DocsPage />);
    expect(within(screen.getByTestId("docs-breadcrumb")).getByText(/Getting started/)).toBeTruthy();
  });

  it("switches pages from the sidebar", () => {
    render(<DocsPage />);
    fireEvent.click(screen.getByTestId("docs-nav-choosing-a-backend"));
    expect(screen.getByTestId("docs-breadcrumb").textContent).toContain("Choosing a backend");
  });

  it("opens ⌘K search, ranks results, and navigates on select", () => {
    render(<DocsPage />);
    // Trigger button opens the modal.
    fireEvent.click(screen.getByTestId("docs-search-trigger"));
    const input = screen.getByTestId("docs-search-input");
    fireEvent.change(input, { target: { value: "troubleshooting" } });
    const results = screen.getAllByTestId("docs-search-result");
    expect(results.length).toBeGreaterThan(0);
    fireEvent.click(results[0]);
    // Modal closed + navigated to the troubleshooting page.
    expect(screen.queryByTestId("docs-search-modal")).toBeNull();
    expect(screen.getByTestId("docs-breadcrumb").textContent).toContain("Troubleshooting");
  });

  it("shows an empty state for a no-match query", () => {
    render(<DocsPage />);
    fireEvent.click(screen.getByTestId("docs-search-trigger"));
    fireEvent.change(screen.getByTestId("docs-search-input"), { target: { value: "zzzznotawordxyz" } });
    expect(screen.getByTestId("docs-search-empty")).toBeTruthy();
  });
});
