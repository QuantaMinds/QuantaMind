/// Collapse models that share a content digest into one entry. A backend may report
/// the same blob once per tag, so importing a model under several tags surfaces
/// it as visual duplicates in the picker (same digest, different names). First
/// occurrence wins. Entries without a digest (llama.cpp GGUF) have no
/// shared identity to merge on and are always kept. Pure.

type WithDigest = { digest?: string };

export function dedupeByDigest<T extends WithDigest>(models: T[]): T[] {
  const seen = new Set<string>();
  const out: T[] = [];
  for (const m of models) {
    if (!m.digest) {
      out.push(m);
      continue;
    }
    if (seen.has(m.digest)) continue;
    seen.add(m.digest);
    out.push(m);
  }
  return out;
}
