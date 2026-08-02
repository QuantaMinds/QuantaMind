import { useEffect, useState } from "react";
import type { ReadinessProfile } from "../../../shared/ipc/eval/readiness";

const numOrNull = (s: string): number | null => {
  const t = s.trim();
  if (t === "") return null;
  const n = Number(t);
  return Number.isFinite(n) && n > 0 ? n : null;
};

function Toggle({ label, hint, checked, onChange }: { label: string; hint: string; checked: boolean; onChange: (b: boolean) => void }) {
  return (
    <label className="flex items-start gap-2.5 cursor-pointer">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="mt-0.5 cursor-pointer" />
      <span className="flex flex-col">
        <span className="text-sm font-semibold text-slate-800">{label}</span>
        <span className="text-[11px] text-slate-500">{hint}</span>
      </span>
    </label>
  );
}

function NumField({ label, hint, value, onChange, suffix }: { label: string; hint: string; value: string; onChange: (s: string) => void; suffix?: string }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-sm font-semibold text-slate-800">{label}</span>
      <div className="flex items-center gap-2">
        <input
          type="number"
          min={1}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder="off"
          className="w-28 bg-white border border-slate-300 rounded-md px-2 py-1 text-sm text-slate-900 outline-none focus:border-slate-400"
        />
        {suffix && <span className="text-xs text-slate-500">{suffix}</span>}
      </div>
      <span className="text-[11px] text-slate-500">{hint}</span>
    </label>
  );
}

/// A real editor for the active profile's gates. Writes the updated profile back
/// to disk via `save_readiness_profile` (Rust = source of truth) and re-assesses.
export function EditProfileModal({
  profile,
  onSave,
  onClose,
}: {
  profile: ReadinessProfile;
  onSave: (p: ReadinessProfile) => Promise<void>;
  onClose: () => void;
}) {
  const [name, setName] = useState(profile.name);
  const [minPassPct, setMinPassPct] = useState(Math.round(profile.min_pass_k * 100));
  const [forbidLoop, setForbidLoop] = useState(profile.forbid_infinite_loop);
  const [forbidHall, setForbidHall] = useState(profile.forbid_hallucinated_completion);
  const [requireVram, setRequireVram] = useState(profile.require_full_vram);
  const [requireNative, setRequireNative] = useState(profile.require_native_fc);
  const [maxSteps, setMaxSteps] = useState(profile.max_avg_steps?.toString() ?? "");
  const [maxLatency, setMaxLatency] = useState(profile.max_ms_per_step?.toString() ?? "");
  const [minCtx, setMinCtx] = useState(profile.min_context_tokens?.toString() ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    const updated: ReadinessProfile = {
      ...profile,
      name: name.trim() || profile.name,
      min_pass_k: Math.min(1, Math.max(0, minPassPct / 100)),
      max_avg_steps: numOrNull(maxSteps),
      max_ms_per_step: numOrNull(maxLatency),
      min_context_tokens: numOrNull(minCtx),
      forbid_infinite_loop: forbidLoop,
      forbid_hallucinated_completion: forbidHall,
      require_full_vram: requireVram,
      require_native_fc: requireNative,
    };
    try {
      await onSave(updated);
      onClose();
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  };

  return (
    <div
      role="presentation"
      onClick={onClose}
      className="fixed inset-0 z-50 bg-black/40 flex items-center justify-center p-4"
      data-testid="edit-profile-modal"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Edit profile thresholds"
        onClick={(e) => e.stopPropagation()}
        className="bg-white rounded-xl shadow-2xl w-[30rem] max-w-[92vw] p-5 space-y-4 border border-slate-200 max-h-[90vh] overflow-y-auto"
      >
        <div className="flex items-center justify-between border-b border-slate-100 pb-3">
          <h3 className="text-sm font-bold text-slate-900">Edit Profile Thresholds</h3>
          <span className="text-[11px] text-slate-400 font-mono">{profile.id}</span>
        </div>

        <label className="flex flex-col gap-1">
          <span className="text-sm font-semibold text-slate-800">Profile name</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            data-testid="edit-profile-name"
            className="bg-white border border-slate-300 rounded-md px-2 py-1 text-sm text-slate-900 outline-none focus:border-slate-400"
          />
        </label>

        <label className="flex flex-col gap-1">
          <span className="text-sm font-semibold text-slate-800">Min Pass^k</span>
          <div className="flex items-center gap-2">
            <input
              type="number"
              min={0}
              max={100}
              value={minPassPct}
              onChange={(e) => setMinPassPct(Math.max(0, Math.min(100, +e.target.value)))}
              data-testid="edit-profile-minpassk"
              className="w-28 bg-white border border-slate-300 rounded-md px-2 py-1 text-sm text-slate-900 outline-none focus:border-slate-400"
            />
            <span className="text-xs text-slate-500">% — hard gate: below this (or unmeasured) → NotReady</span>
          </div>
        </label>

        <div className="grid grid-cols-1 gap-3 border-t border-slate-100 pt-3">
          <Toggle label="Forbid infinite loops" hint="Any loop-capped run → NotReady" checked={forbidLoop} onChange={setForbidLoop} />
          <Toggle label="Forbid hallucinated done" hint="Any fake-“done” completion → NotReady" checked={forbidHall} onChange={setForbidHall} />
          <Toggle label="Require full VRAM fit" hint="Partial offload (or unmeasured) → NotReady. Local backends only." checked={requireVram} onChange={setRequireVram} />
          <Toggle label="Require native FC" hint="Native tool-calling must be supported + measured." checked={requireNative} onChange={setRequireNative} />
        </div>

        <div className="grid grid-cols-1 gap-3 border-t border-slate-100 pt-3">
          <NumField label="Max avg steps" hint="Soft target: over this → Conditional. Blank = off." value={maxSteps} onChange={setMaxSteps} suffix="steps" />
          <NumField label="Max latency" hint="Soft target: over this → Conditional. Blank = off." value={maxLatency} onChange={setMaxLatency} suffix="ms/step" />
          <NumField label="Min context tokens" hint="Hard gate when set: below (or unmeasured) → NotReady. Blank = off." value={minCtx} onChange={setMinCtx} suffix="tokens" />
        </div>

        {error && <div className="text-xs text-red-600">{error}</div>}

        <div className="flex justify-end gap-2 pt-2 border-t border-slate-100">
          <button
            type="button"
            onClick={onClose}
            className="px-3 py-1.5 rounded-md text-sm text-slate-600 hover:bg-slate-100 transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => void handleSave()}
            disabled={saving}
            data-testid="edit-profile-save"
            className="px-3 py-1.5 rounded-md text-sm font-semibold text-white bg-blue-600 hover:bg-blue-500 disabled:opacity-50"
          >
            {saving ? "Saving…" : "Save thresholds"}
          </button>
        </div>
      </div>
    </div>
  );
}
