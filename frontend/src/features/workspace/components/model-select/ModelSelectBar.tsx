import { useNavStore } from "../../../../shared/state/navStore";

/// Workspace model affordances. The picker itself lives in the global header
/// (ModelSelector); this keeps the "Add Model" shortcut to the Models tab.
export function ModelSelectBar() {
  const goToModels = useNavStore((s) => s.setTopView);

  return (
    <div className="flex items-center justify-end">
      <button
        type="button"
        onClick={() => goToModels("models")}
        className="border rounded px-3 py-1 text-sm hover:bg-gray-50 shrink-0"
        data-testid="add-model-button"
      >
        Add Model
      </button>
    </div>
  );
}
