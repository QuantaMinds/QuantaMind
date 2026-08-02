import { useEffect, useState } from "react";
import {
  getUserSettings,
  setUserSettings,
  type UserSettings,
} from "../../../shared/ipc/settings/userSettings";
import { rawMessage } from "../../../shared/ipc/core/error";

type SaveState = "idle" | "saving" | "saved" | "error";

/// The price basis behind the Test-run dollar figures.
///
/// Deliberately has **no default**. Until you declare an hourly rate, every cost
/// figure reads "n/a (no price basis)" rather than $0.00 — a fabricated price
/// understates a real bill, which is worse than showing none.
export function CostBasisSection() {
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [save, setSave] = useState<SaveState>("idle");
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    getUserSettings()
      .then(setSettings)
      .catch((e) => console.error("settings load failed:", e));
  }, []);

  const update = (patch: Partial<UserSettings>) => {
    setSettings((s) => (s ? { ...s, ...patch } : s));
    setSave("idle");
  };

  /// An empty box means "no price", not zero — so it clears the field rather than
  /// storing 0, which would render a real run as free.
  const num = (raw: string): number | null => {
    const t = raw.trim();
    if (t === "") return null;
    const n = Number(t);
    return Number.isFinite(n) && n > 0 ? n : null;
  };

  const persist = async () => {
    if (!settings) return;
    setSave("saving");
    setSaveError(null);
    try {
      await setUserSettings(settings);
      setSave("saved");
    } catch (e) {
      console.error("settings save failed:", e);
      setSaveError(rawMessage(e));
      setSave("error");
    }
  };

  if (!settings) {
    return (
      <p className="text-sm text-gray-500" data-testid="cost-basis-loading">
        Loading…
      </p>
    );
  }

  return (
    <section className="border rounded p-4 space-y-3" data-testid="cost-basis-section">
      <div>
        <h3 className="text-sm font-semibold">Run cost basis</h3>
        <p className="text-xs text-gray-600 mt-1">
          What this machine's accelerator costs per hour. Used to price a finished test run —
          per task, and per correct answer. Leave it blank and the cost figures read
          <span className="font-mono"> n/a</span> rather than a made-up number.
        </p>
      </div>

      <div className="flex flex-wrap gap-4">
        <label className="flex flex-col gap-1 text-xs">
          <span className="text-gray-600">GPU price (USD / hour)</span>
          <input
            type="number"
            min="0"
            step="0.01"
            inputMode="decimal"
            placeholder="e.g. 0.98"
            data-testid="cost-gpu-hourly"
            value={settings.gpu_hourly_usd ?? ""}
            onChange={(e) => update({ gpu_hourly_usd: num(e.target.value) })}
            className="border rounded px-2 py-1 w-36 text-sm"
          />
        </label>
        <label className="flex flex-col gap-1 text-xs">
          <span className="text-gray-600">Utilization (1.0 = whole card)</span>
          <input
            type="number"
            min="0"
            max="1"
            step="0.05"
            inputMode="decimal"
            placeholder="1.0"
            data-testid="cost-utilization"
            value={settings.cost_utilization ?? ""}
            onChange={(e) => update({ cost_utilization: num(e.target.value) })}
            className="border rounded px-2 py-1 w-36 text-sm"
          />
        </label>
      </div>

      {/* The caveat travels with the setting, not just the output: whoever types a
          price should know what the resulting number does and doesn't claim. */}
      <p className="text-xs text-gray-500">
        The figures are an <strong>upper bound</strong>: a test run has the accelerator to
        itself, while production batches concurrent requests and amortizes the same fixed cost
        across them. Runs cut short at the output cap are excluded from every figure and counted
        separately.
      </p>

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => void persist()}
          disabled={save === "saving"}
          data-testid="cost-basis-save"
          className="border rounded px-3 py-1 text-sm hover:bg-gray-50 disabled:opacity-50"
        >
          {save === "saving" ? "Saving…" : "Save"}
        </button>
        {save === "saved" && (
          <span className="text-xs text-green-700" data-testid="cost-basis-saved">
            Saved
          </span>
        )}
        {save === "error" && (
          <span role="alert" className="text-xs text-red-600" data-testid="cost-basis-error">
            {saveError}
          </span>
        )}
      </div>
    </section>
  );
}
