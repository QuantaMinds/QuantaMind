/// Friendly label for a model in pickers: `display_name` when present (some backends
/// stores its on-disk path as `name` for wire-id matching and the HF repo
/// here), else the `name`. Selection and wire calls keep using `name`.
export function modelLabel(m: { name: string; display_name?: string }): string {
  return m.display_name ?? m.name;
}
