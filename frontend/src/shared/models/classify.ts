/// Detect whether a model is embedding-only (it can't generate).
/// Used to hide embedding models from the Workspace picker and the
/// Compare multi-select — they would 400 on a generate request and the
/// UI has nowhere useful to send their output yet.

export type ModelLike = { name: string; family?: string };

const EMBEDDING_FAMILIES = new Set(["bert", "nomic-bert"]);

export function isEmbeddingModel(m: ModelLike): boolean {
  const family = (m.family ?? "").toLowerCase();
  if (EMBEDDING_FAMILIES.has(family)) return true;
  const name = m.name.toLowerCase();
  if (name.includes("embed")) return true;
  if (/^bge[-:](m3|small|base|large)/.test(name)) return true;
  if (name.startsWith("all-minilm")) return true;
  return false;
}

/// Name markers of reasoning/"thinking" model families that emit a <think> scratchpad.
/// Conservative substring match — used only to pre-set the thinking toggle's DEFAULT, which
/// the user can always override. Deliberately excludes non-reasoning lines (qwen2.x, gemma,
/// llama, mistral-non-magistral). Extend as new reasoning families ship.
const THINKING_MARKERS = [
  "qwen3", // Qwen3.x family (thinking mode) — NOT qwen2 / qwen2.5
  "qwq",
  "deepseek-r1",
  "magistral", // Mistral's reasoning line
  "gpt-oss", // harmony reasoning
  "exaone-deep",
  "phi-4-reasoning",
  "phi4-reasoning",
  "marco-o1",
  "cogito",
  "reasoning",
  "reasoner",
  "thinking",
];

/// Best-effort guess: is this likely a reasoning model? Drives the DEFAULT state of the
/// per-model thinking toggle so a user doesn't have to know — an explicit toggle still wins.
export function isLikelyThinkingModel(name: string): boolean {
  const n = name.toLowerCase();
  return THINKING_MARKERS.some((m) => n.includes(m));
}
