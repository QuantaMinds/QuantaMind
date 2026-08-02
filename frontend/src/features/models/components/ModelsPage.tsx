import { useModelStore, type TabId } from "../state/modelStore";
import { useNavStore } from "../../../shared/state/navStore";
import { useHotkey } from "../../../shared/ui/useHotkey";
import { HuggingFaceTab } from "./tabs/HuggingFaceTab";
import { LocalFileTab } from "./tabs/LocalFileTab";

type Tab = { id: TabId; label: string };
const TABS: Tab[] = [
  { id: "huggingface", label: "Hugging Face" },
  { id: "local", label: "Local File" },
];

const subTabClass = (active: boolean) =>
  `px-4 py-2 text-sm ${active ? "border-b-2 border-blue-600 font-medium" : "text-gray-600 hover:text-ink"}`;

export function ModelsPage() {
  const activeTab = useModelStore((s) => s.activeTab);
  const setActiveTab = useModelStore((s) => s.setActiveTab);
  const onModels = useNavStore((s) => s.topView) === "models";
  // Cmd+1/2 switch sub-tabs, only while the Models top tab is active.
  useHotkey("mod+1", () => setActiveTab(TABS[0].id), onModels);
  useHotkey("mod+2", () => setActiveTab(TABS[1].id), onModels);

  return (
    <section data-testid="page-models" className="flex flex-col gap-3 h-full">
      <h2 className="text-lg font-semibold">Add Model</h2>
      <nav className="flex border-b" role="tablist" data-testid="models-tablist">
        {TABS.map((t) => (
          <button
            key={t.id}
            role="tab"
            type="button"
            aria-selected={activeTab === t.id}
            onClick={() => setActiveTab(t.id)}
            className={subTabClass(activeTab === t.id)}
            data-testid={`models-tab-${t.id}`}
          >
            {t.label}
          </button>
        ))}
      </nav>
      <main className="flex-1 overflow-auto">
        {activeTab === "huggingface" && <HuggingFaceTab />}
        {activeTab === "local" && <LocalFileTab />}
      </main>
    </section>
  );
}
