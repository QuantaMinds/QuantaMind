import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

// Stub the sections so SettingsPage renders without their IPC/hardware dependencies — this test
// only asserts composition (the update checker now lives in Settings, moved from the old Help tab).
vi.mock("../HardwareSection", () => ({ HardwareSection: () => <div data-testid="hardware-section" /> }));
vi.mock("../RemoteBackendsSection", () => ({ RemoteBackendsSection: () => <div data-testid="remote-section" /> }));
vi.mock("../../../updater/components/UpdateChecker", () => ({
  UpdateChecker: () => <div data-testid="update-checker" />,
}));

import { SettingsPage } from "../SettingsPage";

describe("SettingsPage", () => {
  it("hosts the app update checker (re-homed from the removed Help tab)", () => {
    render(<SettingsPage />);
    expect(screen.getByTestId("hardware-section")).toBeInTheDocument();
    expect(screen.getByTestId("remote-section")).toBeInTheDocument();
    expect(screen.getByTestId("update-checker")).toBeInTheDocument();
  });
});
