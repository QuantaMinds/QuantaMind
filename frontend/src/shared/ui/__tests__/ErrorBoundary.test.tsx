import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi, afterEach } from "vitest";
import { ErrorBoundary } from "../ErrorBoundary";

// A child that throws while `throwers` says its label should throw — lets a test
// flip a throw off and assert the boundary's Retry actually remounts it.
const throwers: Record<string, boolean> = {};
function Boom({ id }: { id: string }) {
  if (throwers[id]) throw new Error(`boom-${id}`);
  return <div data-testid={`ok-${id}`}>ok-{id}</div>;
}

afterEach(() => {
  for (const k of Object.keys(throwers)) delete throwers[k];
  vi.restoreAllMocks();
});

describe("ErrorBoundary", () => {
  it("catches a render throw, LOGS it (never swallows), and shows the fallback", () => {
    throwers.a = true;
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary label="Panel A">
        <Boom id="a" />
      </ErrorBoundary>,
    );
    expect(screen.getByTestId("error-boundary-fallback")).toHaveTextContent("boom-a");
    expect(screen.getByTestId("error-boundary-fallback")).toHaveTextContent("Panel A");
    // The real throw must reach the console — a hidden card without a log is worse for debugging.
    expect(errSpy).toHaveBeenCalled();
    expect(errSpy.mock.calls.some((c) => c.some((a) => a instanceof Error && a.message === "boom-a"))).toBe(true);
  });

  it("Retry remounts the subtree fresh (recovers once the throw is gone)", () => {
    throwers.b = true;
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary label="Panel B">
        <Boom id="b" />
      </ErrorBoundary>,
    );
    expect(screen.getByTestId("error-boundary-fallback")).toBeInTheDocument();
    throwers.b = false; // the underlying cause is fixed
    fireEvent.click(screen.getByTestId("error-action")); // ErrorCard's Retry button
    expect(screen.getByTestId("ok-b")).toBeInTheDocument(); // remounted, renders normally
    expect(screen.queryByTestId("error-boundary-fallback")).not.toBeInTheDocument();
  });

  it("scopes the failure to one panel — a sibling boundary stays alive", () => {
    throwers.x = true; // x throws, y is fine
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <>
        <ErrorBoundary label="X"><Boom id="x" /></ErrorBoundary>
        <ErrorBoundary label="Y"><Boom id="y" /></ErrorBoundary>
      </>,
    );
    expect(screen.getByTestId("error-boundary-fallback")).toHaveTextContent("boom-x");
    expect(screen.getByTestId("ok-y")).toBeInTheDocument(); // the other panel is untouched
  });
});
