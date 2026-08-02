/// Classify an HF GGUF filename so we can refuse to install variants
/// the server can't register as standalone models. Triggered the day a user
/// installed `mmproj-gemma-4-26b-a4b-it-bf16.gguf`: download succeeded,
/// the install stream closed without a `success` chunk, and the model list
/// stayed empty. Better to never let the click happen than to detect
/// after the fact.

export type VariantKind = "model" | "projection" | "adapter";

export interface VariantClassification {
  kind: VariantKind;
  /// Label shown in place of the Install button (short, fits in a table cell).
  label?: string;
  /// Longer hint surfaced as the cell's title=… so a hover explains why.
  reason?: string;
}

export function classifyHfVariant(filename: string): VariantClassification {
  const lower = filename.toLowerCase();
  if (lower.includes("mmproj")) {
    return {
      kind: "projection",
      label: "Projection layer",
      reason: "Multimodal projection (mmproj) — pairs with a base vision/LLM model, not standalone. Look for a full-model variant in the same repo.",
    };
  }
  if (/\blora\b|-lora-|_lora_|-adapter\b|_adapter_/.test(lower)) {
    return {
      kind: "adapter",
      label: "LoRA / adapter",
      reason: "Adapter / LoRA fine-tune — needs a base model to apply on top of; it can't be installed alone.",
    };
  }
  return { kind: "model" };
}
