/// Translate a raw IPC install error (AppError shape `{kind, message}`
/// or a plain string) into a UI-facing explanation with a next step
/// the user can act on. The raw `formatIpcError` output is diagnostic;
/// this one is for the user. Callers wire this into the install hooks
/// (`useHfInstall`) where the error renders beside the download.

type IpcErr = { kind?: string; message?: string };

function parseErr(raw: unknown): { kind: string; msg: string } {
  if (typeof raw === "string") return { kind: "", msg: raw };
  if (raw && typeof raw === "object") {
    const e = raw as IpcErr;
    return { kind: String(e.kind ?? ""), msg: String(e.message ?? "") };
  }
  return { kind: "", msg: String(raw ?? "") };
}

export function friendlyInstallError(raw: unknown): string {
  const { kind, msg } = parseErr(raw);
  const lower = msg.toLowerCase();

  if (kind === "auth_required") {
    return "This Hugging Face repo is gated. Approve access on huggingface.co — QuantaMind doesn't carry HF tokens yet.";
  }
  if (lower.includes("rate limited") || lower.includes("http 429")) {
    return "Hugging Face is rate-limiting requests. Wait a minute and try again.";
  }
  if (lower.includes("invalid model name")) {
    return "That model name isn't valid (the name rules are strict on length/format). Try a different variant or a more standardly-named repo.";
  }
  if (kind === "not_found" || lower.includes("file does not exist") || lower.includes("model not found")) {
    return "That model wasn't found. Double-check the repo and filename on huggingface.co.";
  }
  if (kind === "truncated" || lower.includes("gguf truncated")) {
    return "The download was incomplete — this isn't the full model file. Check your connection and try downloading it again.";
  }
  if (lower.includes("big-endian") || lower.includes("bad magic")
      || lower.includes("invalid gguf") || lower.includes("unsupported quant")
      || lower.includes("unsupported architecture")) {
    return "This GGUF can't be loaded. It looks corrupted, big-endian, or an unsupported format/architecture. Try a different variant.";
  }
  if (kind === "timeout" || lower.includes("timed out")) {
    return "The install timed out. Check your network and try again.";
  }
  if (lower.includes("hf http") || lower.includes("hf rate")) {
    return `Hugging Face returned an error — ${msg}`;
  }
  return msg || "Install failed for an unknown reason.";
}
