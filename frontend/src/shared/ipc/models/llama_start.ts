import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { InstalledModelInfoSchema, type InstalledModelInfo } from "./storage";

export const LlamaStartResultSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("already_running") }),
  z.object({
    status: z.literal("started"),
    pid: z.number().int().nonnegative(),
    port: z.number().int().nonnegative(),
    // Present ONLY when the backend applied a hardware constraint at launch (flash attention /
    // Q8 KV cache / capped context on a memory-tight host). The UI surfaces it so the user
    // learns what was detected and how the server is running safely.
    note: z.string().optional(),
  }),
  z.object({ status: z.literal("not_bundled"), note: z.string() }),
  z.object({ status: z.literal("start_failed"), error: z.string() }),
]);
export type LlamaStartResult = z.infer<typeof LlamaStartResultSchema>;

/// Start the llama-server sidecar on a specific GGUF (one model at a time).
/// `numCtx` is the user's "Context window" param: it drives the launch `-c`
/// (llama.cpp fixes context at launch), so changing it relaunches the server.
/// `null`/omitted ⇒ the safe default (GGUF context capped for KV-cache safety).
export async function startLlamaServer(
  modelPath: string,
  numCtx?: number | null,
): Promise<LlamaStartResult> {
  const raw = await invoke("start_llama_server", { modelPath, numCtx: numCtx ?? null });
  return LlamaStartResultSchema.parse(raw);
}

export async function stopLlamaServer(): Promise<void> {
  await invoke("stop_llama_server");
}

/// One-time spawn readout for the running llama-server: the model's on-disk
/// footprint and the spawn→ready load time. `null` when no server is up (the UI
/// then shows nothing rather than a fabricated phase). `model_bytes` is null when
/// the GGUF couldn't be stat'd.
export const LlamaServerInfoSchema = z
  .object({
    model_bytes: z.number().int().nonnegative().nullable(),
    load_ms: z.number().int().nonnegative(),
  })
  .nullable();
export type LlamaServerInfo = z.infer<typeof LlamaServerInfoSchema>;

export async function llamaServerInfo(): Promise<LlamaServerInfo> {
  return LlamaServerInfoSchema.parse(await invoke("llama_server_info"));
}

/// The RUNNING llama-server's model path + launch context window (app-spawned state
/// first, `/props` probe for external servers). llama.cpp pins context at launch, so
/// THIS — not the model's GGUF maximum — is the deepest window a Context Stress Test
/// can actually measure against. `null` = nothing running.
export const LlamaWindowSchema = z.object({ path: z.string(), ctx: z.number().int().nonnegative() }).nullable();
export type LlamaWindow = z.infer<typeof LlamaWindowSchema>;

export async function llamaRunningWindow(): Promise<LlamaWindow> {
  return LlamaWindowSchema.parse(await invoke("llama_running_window"));
}

/// GGUF models discovered on disk for the llama.cpp backend.
export async function listLlamaModels(): Promise<InstalledModelInfo[]> {
  const raw = await invoke("list_llama_models");
  return z.array(InstalledModelInfoSchema).parse(raw);
}

/// Delete a llama.cpp model's GGUF file from the shared weights folder.
export async function deleteLlamaModel(path: string): Promise<void> {
  await invoke("delete_llama_model", { path });
}
