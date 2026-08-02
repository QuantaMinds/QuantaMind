import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("../../../../shared/ipc/settings/userSettings", () => ({
  getUserSettings: vi.fn(),
  setUserSettings: vi.fn(),
}));

import { getUserSettings, setUserSettings } from "../../../../shared/ipc/settings/userSettings";
import { CostBasisSection } from "../CostBasisSection";

const base = { first_run_complete: true, community_prompt_shown: false };

beforeEach(() => {
  vi.mocked(setUserSettings).mockReset().mockResolvedValue(undefined);
  vi.mocked(getUserSettings).mockReset().mockResolvedValue({ ...base } as never);
});

describe("CostBasisSection", () => {
  it("starts blank — there is no default price", async () => {
    render(<CostBasisSection />);
    const price = (await screen.findByTestId("cost-gpu-hourly")) as HTMLInputElement;
    expect(price.value).toBe("");
  });

  it("saves a declared hourly price", async () => {
    render(<CostBasisSection />);
    fireEvent.change(await screen.findByTestId("cost-gpu-hourly"), { target: { value: "0.98" } });
    fireEvent.click(screen.getByTestId("cost-basis-save"));
    await waitFor(() =>
      expect(setUserSettings).toHaveBeenCalledWith(expect.objectContaining({ gpu_hourly_usd: 0.98 })),
    );
  });

  /// Clearing the box means "no price", NOT zero — storing 0 would render a real
  /// run as free instead of unpriced.
  it("clearing the price stores null, never 0", async () => {
    vi.mocked(getUserSettings).mockResolvedValue({ ...base, gpu_hourly_usd: 0.98 } as never);
    render(<CostBasisSection />);
    const price = await screen.findByTestId("cost-gpu-hourly");
    await waitFor(() => expect((price as HTMLInputElement).value).toBe("0.98"));
    fireEvent.change(price, { target: { value: "" } });
    fireEvent.click(screen.getByTestId("cost-basis-save"));
    await waitFor(() =>
      expect(setUserSettings).toHaveBeenCalledWith(expect.objectContaining({ gpu_hourly_usd: null })),
    );
  });

  it("a zero or negative price is treated as no price, not as free compute", async () => {
    render(<CostBasisSection />);
    fireEvent.change(await screen.findByTestId("cost-gpu-hourly"), { target: { value: "0" } });
    fireEvent.click(screen.getByTestId("cost-basis-save"));
    await waitFor(() =>
      expect(setUserSettings).toHaveBeenCalledWith(expect.objectContaining({ gpu_hourly_usd: null })),
    );
  });

  it("carries the upper-bound caveat next to the input, not just in the output", async () => {
    render(<CostBasisSection />);
    expect(await screen.findByText(/upper bound/i)).toBeInTheDocument();
  });

  it("surfaces a save failure instead of silently claiming saved", async () => {
    vi.mocked(setUserSettings).mockRejectedValue(new Error("disk full"));
    render(<CostBasisSection />);
    fireEvent.change(await screen.findByTestId("cost-gpu-hourly"), { target: { value: "1.5" } });
    fireEvent.click(screen.getByTestId("cost-basis-save"));
    expect(await screen.findByTestId("cost-basis-error")).toHaveTextContent(/disk full/);
    expect(screen.queryByTestId("cost-basis-saved")).not.toBeInTheDocument();
  });
});
