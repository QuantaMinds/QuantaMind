/// Derive the installed model name from an HF GGUF filename. When a
/// quantization is supplied the name is encoded as `<base>:<quant>` to
/// satisfy the name rules, which reject names that look like an untagged
/// blob with embedded dots ("invalid model name"). The trailing quant
/// suffix on the base (e.g. `-Q4_K_M`, `.Q4_K_M`) is stripped so the
/// quant doesn't appear twice. The HF detail page and the install
/// dispatch both call this so they stay in sync.
///
/// Some HF repos publish GGUFs inside a subdirectory (e.g.
/// `bert-bge-small/ggml-model-f16-big-endian.gguf`). We use only the
/// basename — the subdirectory inflates the name past the
/// internal length/pattern thresholds (which produce a generic
/// "invalid model name" 400) and the basename is what carries the
/// meaningful identity. Any other illegal char in the resulting
/// segment (whitespace, quotes, nulls, residual separators) is
/// sanitized to `-`, keeping the `:` tag separator intact.

const ILLEGAL_NAME_CHARS = /[\/\\\0"' \t\n]/g;

function sanitizeNameSegment(s: string): string {
  return s.replace(ILLEGAL_NAME_CHARS, "-");
}

function basename(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash >= 0 ? path.slice(slash + 1) : path;
}

export function hfVariantModelName(filename: string, quantization?: string): string {
  const stem = basename(filename).replace(/\.gguf$/i, "").toLowerCase();
  const base = sanitizeNameSegment(stem);
  if (!quantization) return base;
  const q = sanitizeNameSegment(quantization.toLowerCase());
  const stripped = base.replace(new RegExp(`[._-]${q}$`), "");
  return `${stripped}:${q}`;
}
