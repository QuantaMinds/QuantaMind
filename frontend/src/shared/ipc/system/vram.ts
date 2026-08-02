import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

// One currently-resident model. size_vram is the VRAM portion
// of the size total; the rest is offloaded to system RAM. context_length is a
// reported when the full-context KV is preallocated into VRAM.
export const LoadedModelSchema = z.object({
  name: z.string(),
  size_bytes: z.number().int().nonnegative(),
  size_vram_bytes: z.number().int().nonnegative(),
  context_length: z.number().int().nonnegative().nullable().optional(),
});
export type LoadedModel = z.infer<typeof LoadedModelSchema>;

const ListSchema = z.array(LoadedModelSchema);

export async function loadedModels(): Promise<LoadedModel[]> {
  try {
    const raw = await invoke("get_loaded_models");
    const r = ListSchema.safeParse(raw);
    if (!r.success) {
      console.error("invalid get_loaded_models payload", r.error.issues);
      return [];
    }
    return r.data;
  } catch (e) {
    // VRAM is a best-effort panel; never let it break the Inspector.
    console.error("get_loaded_models failed", e);
    return [];
  }
}
