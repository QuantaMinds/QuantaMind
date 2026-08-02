import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const PathValidationSchema = z.object({
  exists: z.boolean(),
  is_dir: z.boolean(),
  writable: z.boolean(),
  free_bytes: z.number().int().nonnegative(),
  total_bytes: z.number().int().nonnegative(),
  sufficient: z.boolean(),
});
export type PathValidation = z.infer<typeof PathValidationSchema>;

export async function validateStoragePath(path: string): Promise<PathValidation> {
  return PathValidationSchema.parse(await invoke("validate_storage_path", { path }));
}
