import { AnalysisTab } from "./AnalysisTab";

/// Hosts the read-only Analysis results. The former Quant sub-tab (quantization
/// comparison) was removed with its feature — right-sizing guidance lives on the
/// Agent Report page instead.
export function AnalysisPage() {
  return (
    <section data-testid="page-analysis" className="flex flex-col gap-3 h-full">
      <main className="flex-1 overflow-auto">
        <AnalysisTab />
      </main>
    </section>
  );
}
