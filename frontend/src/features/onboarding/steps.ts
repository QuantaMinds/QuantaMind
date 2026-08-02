export type OnboardingStep = "server" | "model" | "ready";

/// Which onboarding step to show, derived from live state: get a local server
/// running, then install a model, then scaffold a ready-to-go workspace.
export function currentStep(serverHealthy: boolean | null, modelCount: number): OnboardingStep {
  if (serverHealthy !== true) return "server";
  if (modelCount === 0) return "model";
  return "ready";
}
