import type { BuiltinCollectionInfo } from "../../../shared/ipc/eval/registry";

const TIER_ORDER = ["easy", "medium", "hard", "extreme"] as const;
const TIER_LABEL: Record<string, string> = { easy: "Easy", medium: "Medium", hard: "Hard", extreme: "Extreme" };

/// Render the built-in tiered collections as `<optgroup>`s ordered Easy→Extreme,
/// so every dataset dropdown is organized by tier. The backend offers three per
/// tier, so a group is a short list of domains. Empty tiers are omitted.
export function PresetOptGroups({ presets }: { presets: BuiltinCollectionInfo[] }) {
  return (
    <>
      {TIER_ORDER.map((tier) => {
        const items = presets.filter((p) => p.tier === tier);
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
    </>
  );
}
