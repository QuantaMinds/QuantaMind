import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const HfSearchHitSchema = z.object({
  id: z.string().min(1),
  downloads: z.number().int().nonnegative(),
  likes: z.number().int().nonnegative(),
  tags: z.array(z.string()),
  last_modified: z.string().nullable(),
});
export type HfSearchHit = z.infer<typeof HfSearchHitSchema>;

export const HfRepoFileSchema = z.object({
  path: z.string().min(1),
  size_bytes: z.number().int().nonnegative(),
});
export type HfRepoFile = z.infer<typeof HfRepoFileSchema>;

/// Which backend a repo must be usable by — selects the HF tag the search
/// filters on. Only GGUF repos are runnable now, so this is a single-variant
/// seam kept for a future weight format rather than a user-facing choice.
export type RepoKind = "gguf";

export async function hfSearch(
  query: string,
  limit?: number,
  kind: RepoKind = "gguf",
): Promise<HfSearchHit[]> {
  const raw = await invoke("hf_search", { query, limit, kind });
  return z.array(HfSearchHitSchema).parse(raw);
}

export async function hfRepoFiles(repo: string): Promise<HfRepoFile[]> {
  const raw = await invoke("hf_repo_files", { repo });
  return z.array(HfRepoFileSchema).parse(raw);
}

/// All downloadable files in a repo — used to show the total download size +
/// fit estimate before a download.
export async function hfRepoAllFiles(repo: string): Promise<HfRepoFile[]> {
  const raw = await invoke("hf_repo_all_files", { repo });
  return z.array(HfRepoFileSchema).parse(raw);
}

export const ModelCardSchema = z.object({
  description: z.string(),
  license: z.string().nullable(),
  base_model: z.string().nullable(),
  pipeline_tag: z.string().nullable(),
  tags: z.array(z.string()),
});
export type ModelCard = z.infer<typeof ModelCardSchema>;

/// A repo's structured model card (license, base model, task, tags,
/// description), or null when the repo has none.
export async function hfModelCard(repo: string): Promise<ModelCard | null> {
  const raw = await invoke("hf_model_card", { repo });
  return ModelCardSchema.nullable().parse(raw);
}
