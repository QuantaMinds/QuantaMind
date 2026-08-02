import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../../../shared/ipc/settings/userSettings", () => ({
  getUserSettings: vi.fn().mockResolvedValue({ first_run_complete: false }),
  setUserSettings: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("../../../shared/ipc/system/onboarding", () => ({
  RECOMMENDED_MODEL: "llama3.2:1b",
  scaffoldOnboardingWorkspace: vi.fn().mockResolvedValue("/ws"),
}));

import { OnboardingCoach } from "../components/OnboardingCoach";
import { useOnboardingStore } from "../state/onboardingStore";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useNavStore } from "../../../shared/state/navStore";

beforeEach(() => {
  vi.clearAllMocks();
  useOnboardingStore.setState({ complete: false });
  useBackendStore.setState({ llamaHealthy: true });
  useInstalledModelsStore.setState({ list: [] });
  useNavStore.setState({ topView: "workspace" });
});

describe("OnboardingCoach", () => {
  it("is hidden once onboarding is complete", () => {
    useOnboardingStore.setState({ complete: true });
    render(<OnboardingCoach />);
    expect(screen.queryByTestId("onboarding-coach")).toBeNull();
  });

  it("shows the server step when llama.cpp isn't healthy", () => {
    useBackendStore.setState({ llamaHealthy: false });
    render(<OnboardingCoach />);
    expect(screen.getByTestId("onboarding-server")).toBeTruthy();
  });

  it("shows the model step and sends the user to the Models tab", () => {
    render(<OnboardingCoach />);
    expect(screen.getByTestId("onboarding-model")).toBeTruthy();
    fireEvent.click(screen.getByTestId("onboarding-browse"));
    expect(useNavStore.getState().topView).toBe("models");
  });

  it("shows the ready step and finishes on open", async () => {
    useInstalledModelsStore.setState({ list: [{ name: "llama3.2:1b" }] as never });
    render(<OnboardingCoach />);
    fireEvent.click(screen.getByTestId("onboarding-finish"));
    await waitFor(() => expect(useOnboardingStore.getState().complete).toBe(true));
  });

  it("Skip finishes onboarding", async () => {
    render(<OnboardingCoach />);
    fireEvent.click(screen.getByTestId("onboarding-skip"));
    await waitFor(() => expect(useOnboardingStore.getState().complete).toBe(true));
  });
});
