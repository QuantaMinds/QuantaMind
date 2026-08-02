import { useEffect } from "react";
import { useOnboardingStore } from "../state/onboardingStore";
import { currentStep } from "../steps";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useWorkspacesStore } from "../../workspaces/state/workspaceStore";
import { useNavStore } from "../../../shared/state/navStore";
import { scaffoldOnboardingWorkspace, RECOMMENDED_MODEL } from "../../../shared/ipc/system/onboarding";

const card = "border rounded-lg p-4 bg-blue-50 flex flex-col gap-3";
const primary = "self-start text-sm bg-blue-600 text-white rounded px-3 py-1 hover:bg-blue-700";

export function OnboardingCoach() {
  const complete = useOnboardingStore((s) => s.complete);
  const load = useOnboardingStore((s) => s.load);
  const finish = useOnboardingStore((s) => s.finish);
  const healthy = useBackendStore((s) => s.llamaHealthy);
  const modelCount = useInstalledModelsStore((s) => s.list.length);
  const setView = useNavStore((s) => s.setTopView);

  useEffect(() => { void load(); }, [load]);
  if (complete !== false) return null;
  const step = currentStep(healthy, modelCount);

  const openWorkspace = async () => {
    try {
      const root = await scaffoldOnboardingWorkspace();
      await useWorkspacesStore.getState().open(root);
      const welcome = useWorkspacesStore.getState().tree
        .find((n) => n.kind === "file" && n.name === "welcome.quantamind.yaml");
      if (welcome) await useWorkspacesStore.getState().selectPrompt(welcome.path);
    } catch (e) { console.error("scaffold failed:", e); }
    await finish();
  };

  return (
    <div data-testid="onboarding-coach" className={card}>
      <div className="flex items-center justify-between">
        <h2 className="text-base font-semibold">Welcome to QuantaMind</h2>
        <button type="button" onClick={() => void finish()} data-testid="onboarding-skip" className="text-xs text-gray-500 hover:text-ink">
          Skip setup
        </button>
      </div>
      {step === "server" && (
        <div className="flex flex-col gap-2" data-testid="onboarding-server">
          <p className="text-sm text-gray-700">
            Step 1 of 3 — start llama.cpp, the local engine QuantaMind runs models on. Press ▶ in
            the header; QuantaMind ships the server for you.
          </p>
        </div>
      )}
      {step === "model" && (
        <div className="flex flex-col gap-2" data-testid="onboarding-model">
          <p className="text-sm text-gray-700">Step 2 of 3 — install a model. We recommend <strong>{RECOMMENDED_MODEL}</strong> (small and fast).</p>
          <div className="flex gap-2">
            <button type="button" className={primary} data-testid="onboarding-browse"
              onClick={() => setView("models")}>
              Browse models
            </button>
          </div>
        </div>
      )}
      {step === "ready" && (
        <div className="flex flex-col gap-2" data-testid="onboarding-ready">
          <p className="text-sm text-gray-700">Step 3 of 3 — you're set. We'll create a workspace with a welcome prompt so you can see streaming in action.</p>
          <button type="button" className={primary} data-testid="onboarding-finish" onClick={() => void openWorkspace()}>
            Open my workspace
          </button>
        </div>
      )}
    </div>
  );
}
