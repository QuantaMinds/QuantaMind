import { useEffect } from "react";
import { Workspace } from "./features/workspace/components/Workspace";
import { AnalysisPage } from "./features/compare/components/AnalysisPage";
import { InspectorPage } from "./features/inspector/components/InspectorPage";
import { SettingsPage } from "./features/settings/components/SettingsPage";
import { ModelsPage } from "./features/models/components/ModelsPage";
import { DownloadsPage } from "./features/models/components/DownloadsPage";
import { EvalPage } from "./features/eval/components/EvalPage";
import { AuditPage } from "./features/audit/components/AuditPage";
import { AgentReportPage } from "./features/agentReport/components/AgentReportPage";
import { startInstalledModelsBus } from "./features/models/state/installedModelsBus";
import { useModelSettingsStore } from "./features/models/state/modelSettingsStore";
import { FeedbackButton } from "./features/feedback/components/FeedbackButton";
import { DocsPage } from "./features/docs/components/DocsPage";
import { WorkspaceSidebar } from "./WorkspaceSidebar";
import { useAutoSave } from "./features/workspaces/hooks/useAutoSave";
import { HistoryPanel } from "./features/history/components/HistoryPanel";
import { StartupUpdate } from "./features/updater/components/StartupUpdate";
import { OnboardingCoach } from "./features/onboarding/components/OnboardingCoach";
import { AppHeader } from "./AppHeader";
import { useGlobalHotkeys } from "./appHotkeys";
import { CheatsheetModal } from "./shared/ui/CheatsheetModal";
import { ErrorBoundary } from "./shared/ui/ErrorCard";
import { ToastHost } from "./shared/ui/Toast";
import { useNavStore, type TopView } from "./shared/state/navStore";

const TABS: { id: TopView; label: string }[] = [
   { id: "workspace", label: "Workspace" },
   { id: "compare", label: "Analysis" },
   { id: "inspector", label: "Latency" },
   { id: "eval", label: "Tests" },
   { id: "audit", label: "Audit" },
   { id: "agentReport", label: "Agent Report" },
   { id: "models", label: "Models" },
   { id: "downloads", label: "Downloads" },
   { id: "settings", label: "Settings" },
   { id: "docs", label: "Docs" },
];

const tabClass = (active: boolean) =>
  active
       ? "bg-white text-slate-900 shadow-sm border border-slate-200/60 px-4 py-1.5 text-sm font-semibold rounded-lg transition-all"
       : "px-4 py-1.5 text-sm font-medium text-slate-600 hover:text-slate-900 hover:bg-white/40 rounded-lg transition-all";

export default function App() {
  const view = useNavStore((s) => s.topView);
  const setView = useNavStore((s) => s.setTopView);
  useEffect(() => {
    void startInstalledModelsBus();
    void useModelSettingsStore.getState().load().catch((e) => {
      console.error("model settings load failed:", e);
       });
     }, []);
  useAutoSave();
  useGlobalHotkeys();
  return (
       <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
         {/* Sticky top header + nav — stays fixed while scrolling long pages */}
         <header className="sticky top-0 z-50 bg-white/95 backdrop-blur-sm py-2 -mx-4 sm:-mx-6 lg:-mx-8 px-4 sm:px-6 lg:px-8 border-b border-slate-200/60 overflow-visible">
           <div className="space-y-2">
             <AppHeader />
             <OnboardingCoach />
             <nav className="flex items-center gap-1.5 bg-slate-100/70 p-1.5 rounded-xl border border-slate-200/40 w-fit max-w-full overflow-x-auto scrollbar-none" role="tablist">

               {TABS.map((t) => (
                 <button
                  key={t.id}
                  type="button"
                  role="tab"
                  aria-selected={view === t.id}
                  onClick={() => setView(t.id)}
                  className={tabClass(view === t.id)}
                  data-testid={`view-tab-${t.id}`}
                 >
                   {t.label}
                 </button>
               ))}
             </nav>
           </div>
         </header>

         <main className="py-8 font-sans space-y-6">
           {/* Each tab is wrapped so a render throw degrades to a panel-scoped error
               card — these views are all always-mounted (`hidden=`), so an unbounded
               throw in any one of them would blank the entire app. */}
           <div hidden={view !== "workspace"} data-testid="view-workspace">
             <ErrorBoundary label="Workspace">
               <div className="flex gap-4">
                 <WorkspaceSidebar />
                 <div className="flex-1 min-w-0"><Workspace /></div>
               </div>
             </ErrorBoundary>
           </div>
           <div hidden={view !== "compare"} data-testid="view-compare"><ErrorBoundary label="Analysis"><AnalysisPage /></ErrorBoundary></div>
           <div hidden={view !== "inspector"} data-testid="view-inspector"><ErrorBoundary label="Latency"><InspectorPage /></ErrorBoundary></div>
           <div hidden={view !== "eval"} data-testid="view-eval"><ErrorBoundary label="Tests"><EvalPage /></ErrorBoundary></div>
           <div hidden={view !== "audit"} data-testid="view-audit"><ErrorBoundary label="Audit"><AuditPage /></ErrorBoundary></div>
           <div hidden={view !== "agentReport"} data-testid="view-agentReport"><ErrorBoundary label="Agent Report"><AgentReportPage /></ErrorBoundary></div>
           <div hidden={view !== "models"} data-testid="view-models"><ErrorBoundary label="Models"><ModelsPage /></ErrorBoundary></div>
           <div hidden={view !== "downloads"} data-testid="view-downloads"><ErrorBoundary label="Downloads"><DownloadsPage /></ErrorBoundary></div>
           <div hidden={view !== "settings"} data-testid="view-settings"><ErrorBoundary label="Settings"><SettingsPage /></ErrorBoundary></div>
           <div hidden={view !== "docs"} data-testid="view-docs"><ErrorBoundary label="Docs"><DocsPage /></ErrorBoundary></div>
         </main>

         {/* Overlay/floating components stay outside sticky header */}
         <FeedbackButton />
         <HistoryPanel />
         <CheatsheetModal />
         <StartupUpdate />
         <ToastHost />
       </div>
     );
}
