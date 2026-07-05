import { invoke } from "@tauri-apps/api/core";
import type { HealthStatus } from "./types";
import type { BackendKind } from "../models/storage";

export async function listModels(): Promise<string[]> {
  return invoke<string[]>("list_models");
}

export async function checkOllamaHealth(): Promise<HealthStatus> {
  return invoke<HealthStatus>("check_ollama_health");
}

export async function checkMlxHealth(): Promise<HealthStatus> {
  return invoke<HealthStatus>("check_mlx_health");
}

export async function checkLlamaHealth(): Promise<HealthStatus> {
  return invoke<HealthStatus>("check_llama_health");
}

export async function checkVllmHealth(): Promise<HealthStatus> {
  return invoke<HealthStatus>("check_vllm_health");
}

export async function checkSglangHealth(): Promise<HealthStatus> {
  return invoke<HealthStatus>("check_sglang_health");
}

/// Probe a specific backend's server health. The batch pre-flight uses this to
/// fail fast with a clear message instead of hanging mid-run on a down server.
export function healthFor(backend: BackendKind): Promise<HealthStatus> {
  switch (backend) {
    case "ollama":
      return checkOllamaHealth();
    case "mlx":
      return checkMlxHealth();
    case "vllm":
      return checkVllmHealth();
    case "sglang":
      return checkSglangHealth();
    default:
      return checkLlamaHealth();
  }
}
