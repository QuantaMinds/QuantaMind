export function rawMessage(e: unknown): string {
  // A ZodError's `.message` is a raw JSON issues array — surface the first issue
  // in plain language instead (checked before `Error`, which ZodError extends).
  if (e && typeof e === "object" && "issues" in e && Array.isArray((e as { issues: unknown }).issues)) {
    const first = (e as { issues: Array<{ path?: unknown[]; message?: string }> }).issues[0];
    const field = Array.isArray(first?.path) && first.path.length ? `${first.path.join(".")}: ` : "";
    return `Invalid data — ${field}${first?.message ?? "validation failed"}`;
  }
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) {
    const m = (e as { message: unknown }).message;
    if (typeof m === "string") return m;
  }
  try {
    const json = JSON.stringify(e);
    if (json !== undefined && json !== "{}") return json;
  } catch { /* fall through to String() */ }
  const str = String(e);
  return str === "[object Object]" ? "[unknown error]" : str;
}

function friendly(msg: string): string {
  if (msg.includes("Connection refused")
      || msg.includes("error trying to connect")
      || msg.includes("os error 61")
      || msg.includes("tcp connect error")
      || (msg.includes("error sending request") && msg.includes("localhost:11434"))) {
    return "The local server is not running. Start it and try again.";
  }
  return msg;
}

export function formatIpcError(e: unknown): string {
  return friendly(rawMessage(e));
}
