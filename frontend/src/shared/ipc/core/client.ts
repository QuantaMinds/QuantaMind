import { invoke } from "@tauri-apps/api/core";
import type { HealthStatus } from "./types";
import type { BackendKind } from "../models/storage";

export async function checkLlamaHealth(): Promise<HealthStatus> {
  return invoke<HealthStatus>("check_llama_health");
}

export async function checkVllmHealth(): Promise<HealthStatus> {
  return invoke<HealthStatus>("check_vllm_health");
}

export async function checkSglangHealth(): Promise<HealthStatus> {
  return invoke<HealthStatus>("check_sglang_health");
}

/// The classified outcome of resolving a remote endpoint's credential — distinguishes a
/// rejected key (401/403) from an unreachable server (opposite fixes), plus unconfigured /
/// TLS / wrong-path / server-error. Carries only a REDACTED host — never the key or a URL
/// with embedded credentials.
export type RemoteAuthStatus =
  | "ok"
  | "unconfigured"
  | "unreachable"
  | "tls_error"
  | "unauthorized"
  | "not_found"
  | "server_error";
export type RemoteAuthReport = {
  status: RemoteAuthStatus;
  http_status: number | null;
  host: string;
  /// A key is set but the URL isn't https/loopback, so the key was withheld (never sent over http).
  insecure_key: boolean;
};

export async function checkVllmCredential(): Promise<RemoteAuthReport> {
  return invoke<RemoteAuthReport>("check_vllm_credential");
}

export async function checkSglangCredential(): Promise<RemoteAuthReport> {
  return invoke<RemoteAuthReport>("check_sglang_credential");
}

/// Resolve+validate a remote backend's credential (vLLM/SGLang only). The batch pre-flight
/// uses this to fail fast BEFORE a run with the right message, instead of a mid-run 401.
export function credentialFor(backend: BackendKind): Promise<RemoteAuthReport> {
  return backend === "sglang" ? checkSglangCredential() : checkVllmCredential();
}

/// Probe a specific backend's server health. The batch pre-flight uses this to
/// fail fast with a clear message instead of hanging mid-run on a down server.
export function healthFor(backend: BackendKind): Promise<HealthStatus> {
  switch (backend) {
    case "vllm":
      return checkVllmHealth();
    case "sglang":
      return checkSglangHealth();
    default:
      return checkLlamaHealth();
  }
}
