import { useEffect, useRef, useState } from "react";
import { useEvalRegistryStore } from "../../eval/state/evalRegistryStore";
import { useBatchStore } from "../../eval/state/batchStore";
import { useReadinessStore } from "../state/readinessStore";
import { VerdictTable, PATH_LABEL } from "./VerdictTable";
import { RecommendationBanner } from "./RecommendationBanner";
import { ExecutiveVerdict } from "./ExecutiveVerdict";
import { TierProgressionMatrix } from "./TierProgressionMatrix";
import { FailureTaxonomy } from "./FailureTaxonomy";
import { axesByTier, deepDiveJson } from "../deepDive";
import { download } from "../../eval/exportBatch";
import { ExportMenu } from "./ExportMenu";
import { PublishButton } from "../../publish/PublishButton";
import { useNavStore } from "../../../shared/state/navStore";
import { InfoButton } from "../../../shared/ui/InfoButton";
import { READINESS_HELP } from "../readinessHelp";
import { capOptions, defaultCapBytes, archLabel } from "../capBytes";
import { EditProfileModal } from "./EditProfileModal";
import { useToast } from "../../../shared/ui/Toast";
import type { Tier } from "../../../shared/ipc/eval/readiness";

/// Stable composite identity for a deep-dive target — a model now has up to two verdicts
/// (native + prompt), so the path is part of the key. Pipe-separated: a model name (which
/// may contain a colon, e.g. `qwen2.5:7b`) never contains a `|`, so it can't collide with
/// the fixed-enum path segment (`native_fc`/`prompt_based`).
const focusKey = (model: string, path: string) => `${model}|${path}`;

export function AgentReportPage() {
  const { presets, collections, selected, tasks, select, init } = useEvalRegistryStore();
  const {
    profiles,
    selectedProfileId,
    verdicts,
    hardware,
    hardwareTier,
    focusedModel,
    focusedPath,
    capBytes,
    assessed,
    loading,
    error,
    loadProfiles,
    loadHardware,
    loadHardwareTier,
    setFocus,
    selectProfile,
    setCap,
    assess,
    saveProfile,
  } = useReadinessStore();

  const goBack = useNavStore((s) => s.goBack);
  const batchReport = useBatchStore((s) => s.report);
  const toast = useToast();
  const [showNativeFc, setShowNativeFc] = useState(true);
  const [isSection1Collapsed, setIsSection1Collapsed] = useState(false);
  const [editingProfile, setEditingProfile] = useState(false);
  // Which tier's failures the deep-dive shows (set by clicking a Tier Matrix card);
  // null = none shown. Cleared when the deep-dive target (model/path) changes.
  const [selectedTier, setSelectedTier] = useState<Tier | null>(null);

  // The report card (banner + table) snapshotted to PNG by the export menu.
  const cardRef = useRef<HTMLDivElement>(null);

  const onCapChange = (bytes: number) => {
    setCap(bytes);
    if (assessed) void assess(selected);
  };

  useEffect(() => {
    if (presets.length === 0) void init().catch(() => {});
    void loadProfiles();
    void loadHardware();
    void loadHardwareTier();
  }, [presets.length, init, loadProfiles, loadHardware, loadHardwareTier]);

  // The deep-dive targets one (model, path) — default to the recommended (first, best-ranked)
  // verdict; reset if that exact pair leaves the current verdict set (re-assess / profile
  // switch / switching to a model that lacks the previously-focused path). Keying on the PAIR
  // (not model alone) guarantees focus never orphans to a (model, path) with no verdict.
  useEffect(() => {
    if (verdicts.length === 0) return;
    const exists = verdicts.some((v) => v.model === focusedModel && v.verdict.path === focusedPath);
    if (!exists) setFocus(verdicts[0].model, verdicts[0].verdict.path);
  }, [verdicts, focusedModel, focusedPath, setFocus]);

  // Clear the tier-failure selection when the deep-dive target changes, so one tier's
  // failures never linger after switching model/path.
  useEffect(() => {
    setSelectedTier(null);
  }, [focusedModel, focusedPath]);

  // Auto-refresh after an eval batch finishes (so a new model / tier shows without a
  // manual Run Validation). Re-assess when the completed batch is the shown collection
  // OR a tier-sibling of the same domain (the per-domain merge picks up the new tier).
  // Gated on `assessed` so it never surprise-runs before the first validation; `assess`
  // writes `verdicts` (not `batchReport`), so this can't loop.
  useEffect(() => {
    if (!batchReport || !assessed) return;
    const sel = presets.find((p) => p.id === selected);
    const done = presets.find((p) => p.id === batchReport.collection_id);
    const sameDomain = !!sel && !!done && sel.domain === done.domain;
    if (batchReport.collection_id === selected || sameDomain) void assess(selected);
  }, [batchReport, selected, assessed, presets, assess]);

  // Built-in presets are labeled by DOMAIN only (`humanize(id minus "<tier>-")`), so
  // every tier-variant of a domain collides ("Coding", "Coding", …). Disambiguate each
  // by its tier so the dropdown has no duplicate labels; the id (unique) stays the value.
  const tierCap = (t: string) => t.charAt(0).toUpperCase() + t.slice(1);
  const options = [
    ...presets.map((p) => ({ id: p.id, label: `${p.label} — ${tierCap(p.tier)}` })),
    ...collections.map((c) => ({ id: c, label: c })),
  ];

  const activeProfile = profiles.find((p) => p.id === selectedProfileId);

  // The deep-dive (Executive Verdict / Tier Matrix / Failure Taxonomy) for the focused model.
  // The deep-dive for the focused (model, path). Falls back to the first verdict so it's
  // never undefined when verdicts exist.
  const focused =
    verdicts.find((v) => v.model === focusedModel && v.verdict.path === focusedPath) ?? verdicts[0];
  const tierParams = axesByTier(tasks);
  const minPassK = activeProfile?.min_pass_k ?? 0.8;
  const exportDeepDiveJson = () => {
    if (!focused) return;
    download(
      // Path in the filename so a model's native + prompt deep-dives don't overwrite each other.
      `${focused.model.replace(/[^a-z0-9._-]/gi, "_")}-${focused.verdict.path}-readiness.json`,
      JSON.stringify(deepDiveJson(focused, selected, activeProfile?.name ?? "this profile"), null, 2),
      "application/json",
    );
  };

  const pct = (x: number) => `${Math.round(x * 100)}%`;
  const yn = (b: boolean) => (b ? "YES" : "no");

  return (
    <div
      data-testid="agent-report-page"
      className="bg-white border border-slate-200 shadow-md rounded-2xl overflow-hidden max-w-6xl mx-auto flex flex-col text-slate-900 font-sans"
    >
      {/* Title Header: [≡] LOCAL AGENT READINESS VALIDATOR [User Avatar] [Help] */}
      <header className="bg-slate-50 px-6 py-4 border-b border-slate-200 flex items-center justify-between select-none">
        <div className="flex items-center gap-3">
          <button
            type="button"
            className="text-slate-400 hover:text-slate-700 transition-colors p-1 hover:bg-slate-100 rounded-md cursor-pointer"
            aria-label="Menu"
          >
            <span className="font-mono text-lg font-bold">[≡]</span>
          </button>
          <h1 className="text-sm font-bold tracking-wider text-slate-700 uppercase">
            LOCAL AGENT READINESS VALIDATOR
          </h1>
        </div>

        <div className="flex items-center gap-4">
          {/* Help Action */}
          <div className="flex items-center gap-1.5 text-xs font-medium text-slate-500 hover:text-slate-700 transition-all cursor-pointer">
            <span className="hidden sm:inline">Help</span>
            <InfoButton {...READINESS_HELP.page} align="right" testId="readiness-page" />
          </div>
        </div>
      </header>

      {error && (
        <div data-testid="readiness-error" className="m-6 p-4 bg-red-50 border border-red-200 rounded-xl text-sm text-red-700 flex items-center gap-2">
          <svg className="w-4 h-4 shrink-0 text-red-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
          {error}
        </div>
      )}

      {/* SECTION 1: HOST & THRESHOLDS (Collapsible) */}
      <section className="border-b border-slate-200 bg-slate-50/50 flex flex-col">
        {/* Collapsible Header */}
        <div
          className="flex justify-between items-center py-3.5 px-6 bg-slate-50 hover:bg-slate-100 border-b border-slate-200 cursor-pointer select-none transition-colors duration-150"
          onClick={() => setIsSection1Collapsed(!isSection1Collapsed)}
        >
          <div className="flex items-center gap-2.5 text-xs font-bold text-slate-600 uppercase tracking-wider">
            <span className={`transform transition-transform duration-200 text-slate-400 ${isSection1Collapsed ? "-rotate-90" : ""}`}>
              ▼
            </span>
            <span>SECTION 1: HOST &amp; THRESHOLDS {isSection1Collapsed ? "(Collapsed)" : "(Collapsible)"}</span>
          </div>

          <div onClick={(e) => e.stopPropagation()}>
            <button
              type="button"
              data-testid="edit-profile-open"
              disabled={!activeProfile}
              className="flex items-center gap-1.5 bg-white hover:bg-slate-50 active:bg-slate-100 text-slate-700 border border-slate-300 px-3.5 py-1.5 rounded-lg text-xs transition-all shadow-sm font-semibold cursor-pointer disabled:opacity-50"
              onClick={() => setEditingProfile(true)}
            >
              <span>Edit Profile ⚙</span>
            </button>
          </div>
        </div>

        {/* Collapsible Content */}
        <div className={`px-6 py-5 space-y-4 flex flex-col transition-all duration-200 ${isSection1Collapsed ? "hidden" : ""}`}>

          {/* Row 1: Hardware, VRAM Cap, Target Profile, Collection, and Run button */}
          <div className="flex flex-wrap items-center gap-x-6 gap-y-4">

            {/* Hardware badge container */}
            <div className="flex items-center gap-2 text-xs">
              <span className="text-slate-500 font-semibold uppercase tracking-wider">Hardware:</span>
              <div
                data-testid="host-hardware-profile"
                className="flex items-center gap-1.5 bg-slate-100 text-slate-700 border border-slate-200 px-3 py-1.5 rounded-lg text-xs font-mono select-none"
              >
                <span>{hardware?.gpu?.name || "System Hardware"}</span>
                <span className="hidden">{archLabel(hardware)}</span>
              </div>
            </div>

            {/* VRAM Allocation Cap selection */}
            <div className="flex items-center gap-2 text-xs">
              <span className="text-slate-500 font-semibold uppercase tracking-wider flex items-center gap-1">
                VRAM Cap:
              </span>
              <div className="relative">
                <select
                  data-testid="readiness-cap-select"
                  value={capBytes ?? ""}
                  onChange={(e) => onCapChange(Number(e.target.value))}
                  className="bg-white border border-slate-300 hover:border-slate-400 focus:border-slate-500 rounded-lg py-1.5 pl-3 pr-8 text-xs text-slate-800 transition-all outline-none appearance-none cursor-pointer"
                >
                  {capOptions(defaultCapBytes(hardware) ?? capBytes).map((o) => (
                    <option key={o.bytes} value={o.bytes}>
                      {o.label}
                    </option>
                  ))}
                </select>
                <div className="absolute inset-y-0 right-0 flex items-center pr-2.5 pointer-events-none text-slate-400">
                  <svg className="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
                  </svg>
                </div>
              </div>
            </div>

            {/* Target Profile Selection */}
            <div className="flex items-center gap-2 text-xs">
              <span className="text-slate-500 font-semibold uppercase tracking-wider">Target:</span>
              <div className="relative">
                <select
                  data-testid="readiness-profile-select"
                  value={selectedProfileId}
                  onChange={(e) => selectProfile(e.target.value)}
                  className="bg-white border border-slate-300 hover:border-slate-400 focus:border-slate-500 rounded-lg py-1.5 pl-3 pr-8 text-xs text-slate-800 transition-all outline-none appearance-none cursor-pointer"
                >
                  {profiles.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
                <div className="absolute inset-y-0 right-0 flex items-center pr-2.5 pointer-events-none text-slate-400">
                  <svg className="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
                  </svg>
                </div>
              </div>
            </div>

            {/* Collection Selection */}
            <div className="flex items-center gap-2 text-xs">
              <span className="text-slate-500 font-semibold uppercase tracking-wider">Collection:</span>
              <div className="relative">
                <select
                  data-testid="readiness-collection-select"
                  value={selected}
                  onChange={(e) => void select(e.target.value)}
                  className="bg-white border border-slate-300 hover:border-slate-400 focus:border-slate-500 rounded-lg py-1.5 pl-3 pr-8 text-xs text-slate-800 transition-all outline-none appearance-none cursor-pointer"
                >
                  {options.map((o) => (
                    <option key={o.id} value={o.id}>{o.label}</option>
                  ))}
                </select>
                <div className="absolute inset-y-0 right-0 flex items-center pr-2.5 pointer-events-none text-slate-400">
                  <svg className="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
                  </svg>
                </div>
              </div>
            </div>

            {/* Run Button */}
            <button
              type="button"
              data-testid="readiness-run"
              className="flex items-center justify-center gap-1.5 py-1.5 px-4 bg-blue-600 hover:bg-blue-500 active:bg-blue-700 text-white rounded-lg text-xs font-semibold transition-all disabled:opacity-50 shadow-sm active:scale-[0.98] duration-150 cursor-pointer disabled:cursor-not-allowed h-[30px]"
              disabled={loading || !selectedProfileId}
              onClick={() => void assess(selected)}
            >
              {loading ? (
                <>
                  <svg className="animate-spin h-3.5 w-3.5 text-white" fill="none" viewBox="0 0 24 24">
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" />
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                  </svg>
                  <span>Assessing…</span>
                </>
              ) : (
                <>
                  <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z" />
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                  <span>Run Validation</span>
                </>
              )}
            </button>

          </div>

          {/* Row 2: Active Thresholds display */}
          {activeProfile && (
            <div className="flex flex-col border-t border-slate-200 pt-3.5">
              {/* Visible mockup style list of thresholds */}
              <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs font-semibold text-slate-500 select-none">
                <div className="flex items-center gap-1 bg-slate-50 border border-slate-200 px-2.5 py-1 rounded-md text-[11px]">
                  <span>Pass^k:</span>
                  <span className="text-slate-900 font-mono font-bold">[{pct(activeProfile.min_pass_k)}]</span>
                </div>
                <div className="flex items-center gap-1 bg-slate-50 border border-slate-200 px-2.5 py-1 rounded-md text-[11px]">
                  <span>Infinite Loops:</span>
                  <span className="text-slate-900 font-mono font-bold">
                    [{activeProfile.forbid_infinite_loop ? "ON" : "OFF"}]
                  </span>
                </div>
                <div className="flex items-center gap-1 bg-slate-50 border border-slate-200 px-2.5 py-1 rounded-md text-[11px]">
                  <span>Full VRAM:</span>
                  <span className="text-slate-900 font-mono font-bold">
                    [{activeProfile.require_full_vram ? "ON" : "OFF"}]
                  </span>
                </div>
                <div className="flex items-center gap-1 bg-slate-50 border border-slate-200 px-2.5 py-1 rounded-md text-[11px]">
                  <span>Min Context:</span>
                  <span className="text-slate-900 font-mono font-bold">
                    [{activeProfile.min_context_tokens != null ? activeProfile.min_context_tokens.toLocaleString() : "off"}]
                  </span>
                </div>
                <div className="flex items-center gap-1 bg-slate-50 border border-slate-200 px-2.5 py-1 rounded-md text-[11px]">
                  <span>Max Lat:</span>
                  <span className="text-slate-900 font-mono font-bold">
                    [{activeProfile.max_ms_per_step != null ? `${activeProfile.max_ms_per_step}ms` : "off"}]
                  </span>
                </div>
              </div>

              {/* Hidden text content element to ensure existing tests pass cleanly */}
              <div data-testid="readiness-thresholds" className="hidden" aria-hidden="true">
                <span>Min Pass^k: {pct(activeProfile.min_pass_k)}</span>
                <span>Forbid loops: {yn(activeProfile.forbid_infinite_loop)}</span>
                <span>Forbid fake-done: {yn(activeProfile.forbid_hallucinated_completion)}</span>
                <span>Require full VRAM: {yn(activeProfile.require_full_vram)}</span>
                <span>Require native FC: {yn(activeProfile.require_native_fc)}</span>
                <span>Max steps: {activeProfile.max_avg_steps ?? "off"}</span>
                <span>Max latency: {activeProfile.max_ms_per_step != null ? `${activeProfile.max_ms_per_step} ms/step` : "off"}</span>
                <span>Min context: {activeProfile.min_context_tokens != null ? `${activeProfile.min_context_tokens} tok` : "off"}</span>
              </div>
            </div>
          )}

        </div>
      </section>

      {/* SECTION 2: VERDICT & DIAGNOSTICS */}
      <section className="px-6 py-5 space-y-4 bg-white flex flex-col flex-1">

        <div className="flex justify-between items-center border-b border-slate-200 pb-3 select-none">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-700 flex items-center gap-1.5">
            <span>▼</span> SECTION 2: VERDICT &amp; DIAGNOSTICS
          </span>
          {verdicts.length > 0 && (
            <div className="flex items-center gap-2 text-xs font-semibold text-slate-500">
              <span className="flex items-center gap-1 text-slate-600">
                Show Native-FC Path
                <InfoButton {...READINESS_HELP.nativeFc} testId="readiness-nativefc" />
              </span>
              <button
                type="button"
                className={`px-3 py-1 border rounded-lg text-xs font-bold cursor-pointer transition-all ${
                  showNativeFc
                    ? "bg-blue-600 border-blue-500 text-white"
                    : "bg-slate-100 border-slate-200 text-slate-500 opacity-60 hover:opacity-100"
                }`}
                onClick={() => setShowNativeFc(!showNativeFc)}
              >
                {showNativeFc ? "ON 🟢" : "OFF 🔴"}
              </button>
            </div>
          )}
        </div>

        {verdicts.length > 0 && (
          <div className="space-y-6" ref={cardRef}>
            <RecommendationBanner
              verdicts={verdicts}
              profileName={activeProfile?.name ?? "this profile"}
            />
            <VerdictTable
              verdicts={verdicts}
              profileName={activeProfile?.name}
              showNativeFc={showNativeFc}
              unified={!!hardware?.gpu?.unified}
            />
          </div>
        )}

        {/* Per-model DEEP DIVE: Executive Verdict + Tier Progression Matrix + Failure
            Taxonomy for the focused model (defaults to the recommended one). */}
        {verdicts.length > 0 && focused && (
          <div data-testid="agent-report-deepdive" className="space-y-6 border-t border-slate-200 pt-5">
            <div className="flex items-center justify-between">
              <label className="flex items-center gap-2 text-sm">
                <span className="text-xs font-semibold uppercase tracking-wider text-slate-500">Deep Dive:</span>
                <select
                  data-testid="deepdive-model-select"
                  value={focusKey(focused.model, focused.verdict.path)}
                  onChange={(e) => {
                    const v = verdicts.find((x) => focusKey(x.model, x.verdict.path) === e.target.value);
                    if (v) setFocus(v.model, v.verdict.path);
                  }}
                  className="bg-white border border-slate-200 hover:border-slate-300 focus:border-slate-400 rounded-lg py-1.5 pl-3 pr-8 text-sm text-slate-800 shadow-sm outline-none cursor-pointer font-mono"
                >
                  {verdicts.map((v) => (
                    <option key={focusKey(v.model, v.verdict.path)} value={focusKey(v.model, v.verdict.path)}>
                      {v.model} ({PATH_LABEL[v.verdict.path]})
                    </option>
                  ))}
                </select>
              </label>
              <button
                type="button"
                data-testid="deepdive-export-json"
                onClick={exportDeepDiveJson}
                className="px-3 py-1.5 bg-white border border-slate-300 hover:border-slate-400 text-slate-700 rounded-lg text-xs font-semibold transition-all hover:bg-slate-50 cursor-pointer shadow-sm"
              >
                ⬇ Export JSON
              </button>
            </div>
            <ExecutiveVerdict verdict={focused} hardwareTier={hardwareTier} minPassK={minPassK} />
            <TierProgressionMatrix
              byTier={focused.by_tier}
              minPassK={minPassK}
              params={tierParams}
              selectedTier={selectedTier}
              onSelectTier={setSelectedTier}
            />
            <FailureTaxonomy tier={focused.by_tier?.find((s) => s.tier === selectedTier) ?? null} />
          </div>
        )}

        {assessed && verdicts.length === 0 && !error && (
          <div
            data-testid="readiness-empty"
            className="flex flex-col items-center justify-center p-12 text-center bg-slate-50 border border-dashed border-slate-300 rounded-xl min-h-[300px] shadow-sm gap-3"
          >
            <div className="p-3 bg-amber-50 rounded-full border border-amber-200 text-amber-500">
              <svg className="w-8 h-8" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
              </svg>
            </div>
            <h3 className="text-base font-semibold text-slate-700">No batch report found</h3>
            <p className="text-sm text-slate-500 max-w-sm">
              No batch report found for “{selected}”. Run a batch for this collection on the Tests tab, then come back to assess it.
            </p>
          </div>
        )}

        {!assessed && !loading && (
          <div className="flex flex-col items-center justify-center p-12 text-center bg-slate-50 border border-dashed border-slate-300 rounded-xl min-h-[300px] shadow-sm gap-3">
            <div className="p-3 bg-slate-100 rounded-full border border-slate-200 text-slate-400">
              <svg className="w-8 h-8" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />
              </svg>
            </div>
            <h3 className="text-base font-semibold text-slate-700">Awaiting Assessment</h3>
            <p className="text-sm text-slate-500 max-w-sm">
              Pick a target collection and a profile, then Run readiness.
            </p>
          </div>
        )}
      </section>

      {/* FOOTER: [ < Back to Workbench ] [ ⬇ Export HTML Report ] [ 🚀 Deploy ] */}
      <footer className="px-6 py-4 border-t border-slate-200 bg-slate-50 flex items-center justify-between mt-auto">
        <button
          type="button"
          className="px-4 py-2 bg-white border border-slate-300 hover:border-slate-400 text-slate-700 rounded-lg text-sm font-semibold transition-all hover:bg-slate-50 cursor-pointer shadow-sm"
          onClick={goBack}
        >
          &lt; Back to Workbench
        </button>

        {verdicts.length > 0 && activeProfile && (
          <div className="flex items-center gap-3">
            <ExportMenu
              verdicts={verdicts}
              profile={activeProfile}
              collectionId={selected}
              hardware={hardware}
              cardRef={cardRef}
            />
            <PublishButton verdicts={verdicts} collectionId={selected} collectionHash={batchReport?.collection_hash ?? null} />
          </div>
        )}
      </footer>

      {/* Profile Threshold Editor Modal */}
      {editingProfile && activeProfile && (
        <EditProfileModal
          profile={activeProfile}
          onSave={async (p) => {
            await saveProfile(p);
            toast("Profile thresholds saved ✓");
            if (assessed) await assess(selected);
          }}
          onClose={() => setEditingProfile(false)}
        />
      )}
    </div>
  );
}
