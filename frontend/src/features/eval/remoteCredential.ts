import type { RemoteAuthReport } from "../../shared/ipc/core/client";

/// The fix message IS the deliverable (works-for-me vs works-for-a-stranger). Map a classified
/// remote-credential outcome to a plain-English, actionable line — or `null` when it's OK to
/// proceed. Never echoes the API key or a full URL (the report already carries only a redacted
/// `host`). A rejected key and an unreachable server get OPPOSITE fixes on purpose.
export function remoteCredentialMessage(backend: "vllm" | "sglang", r: RemoteAuthReport): string | null {
  const label = backend === "sglang" ? "SGLang" : "vLLM";
  const host = r.host || "the configured endpoint";
  switch (r.status) {
    case "ok":
      return null;
    case "unauthorized":
      return r.insecure_key
        ? `${label} rejected the request — and your API key wasn't sent because ${host} is plain http. Switch to an https URL in Settings so the key is actually used.`
        : `${label} rejected your API key — check the key in Settings (or your endpoint's auth config).`;
    case "unreachable":
      return `Couldn't reach the ${label} server at ${host} — start it, or fix the URL in Settings, then re-run.`;
    case "unconfigured":
      return `No ${label} endpoint set — add the server URL in Settings, then re-run.`;
    case "tls_error":
      return `TLS/certificate error connecting to ${host} — check the server's HTTPS certificate (or use http on loopback for local testing).`;
    case "not_found":
      return `${host} answered but has no /v1/models — is this an OpenAI-compatible endpoint? Check the URL/path in Settings.`;
    case "server_error":
      return `${label} returned an error${r.http_status ? ` (HTTP ${r.http_status})` : ""} — check the server, then re-run.`;
    default:
      return `Couldn't validate the ${label} endpoint — check it in Settings, then re-run.`;
  }
}
