import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { TierSchema } from "../eval/readiness";

// GPU/VRAM, best-effort per platform. unified = shared memory (Apple Silicon;
// no separate VRAM pool). available:false → "Not available".
export const GpuInfoSchema = z.object({
  name: z.string().nullable().optional(),
  vram_total_bytes: z.number().int().nonnegative().nullable().optional(),
  vram_free_bytes: z.number().int().nonnegative().nullable().optional(),
  unified: z.boolean(),
  available: z.boolean(),
  // Apple Silicon only: the GPU's MEASURED Metal working-set limit — the unified
  // memory the GPU can actually wire down (~66-75% of RAM), not the whole pool. The
  // context-ceiling meters budget against THIS. null/absent off macOS or unmeasured.
  gpu_working_set_bytes: z.number().int().nonnegative().nullable().optional(),
});
export type GpuInfo = z.infer<typeof GpuInfoSchema>;

export const HardwareSnapshotSchema = z.object({
  total_memory_bytes: z.number().int().nonnegative(),
  available_memory_bytes: z.number().int().nonnegative(),
  is_apple_silicon: z.boolean(),
  // Added in 4.5; optional so existing fixtures (memory-only) stay valid.
  cpu: z.string().optional(),
  physical_cores: z.number().int().nonnegative().nullable().optional(),
  os_name: z.string().nullable().optional(),
  os_version: z.string().nullable().optional(),
  arch: z.string().optional(),
  gpu: GpuInfoSchema.optional(),
  // Nominal Apple-Silicon memory bandwidth (GB/s); null/absent → "Not available".
  estimated_bandwidth_gbps: z.number().int().positive().nullable().optional(),
});
export type HardwareSnapshot = z.infer<typeof HardwareSnapshotSchema>;

export async function getHardwareSnapshot(): Promise<HardwareSnapshot> {
  const raw = await invoke("get_hardware_snapshot");
  return HardwareSnapshotSchema.parse(raw);
}

// Hardware class + recommended difficulty tier (mirror of Rust `HardwareTier`).
// The eval page's tier-`Auto` mode and "HW: …" hint read this; the GB thresholds
// + class→tier policy live in the backend (`hwclass.rs`), never duplicated here.
export const HardwareTierSchema = z.object({
  total_memory_bytes: z.number().int().nonnegative(),
  class: z.string(),
  recommended_tier: TierSchema,
});
export type HardwareTier = z.infer<typeof HardwareTierSchema>;

export async function getHardwareTier(): Promise<HardwareTier> {
  const raw = await invoke("get_hardware_tier");
  return HardwareTierSchema.parse(raw);
}
