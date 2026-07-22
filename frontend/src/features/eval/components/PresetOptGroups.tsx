import type { BuiltinCollectionInfo } from "../../../shared/ipc/eval/registry";

const TIER_ORDER = ["easy", "medium", "hard", "extreme"] as const;
const TIER_LABEL: Record<string, string> = { easy: "Easy", medium: "Medium", hard: "Hard", extreme: "Extreme" };

/// Render the built-in collections as `<optgroup>`s: the capability set ordered
/// Easy→Extreme (three domains per tier), then the Category K safety probes in a group
/// of their own — their JSON tier is Easy (it sets the step budget) but they measure a
/// different axis, so listing them under Easy would misread as a fourth easy domain.
/// Empty groups are omitted.
export function PresetOptGroups({ presets }: { presets: BuiltinCollectionInfo[] }) {
  const safety = presets.filter((p) => p.kind === "safety");
  return (
    <>
      {TIER_ORDER.map((tier) => {
        const items = presets.filter((p) => p.kind !== "safety" && p.tier === tier);
        if (items.length === 0) return null;
        return (
          <optgroup key={tier} label={TIER_LABEL[tier]}>
            {items.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </optgroup>
        );
      })}
      {safety.length > 0 && (
        <optgroup label="Safety &amp; Boundaries">
          {safety.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </optgroup>
      )}
    </>
  );
}
