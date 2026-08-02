import { rawMessage } from "./error";

const DOCS = "https://quantamind.co/docs/troubleshooting";

export type ActionHint = "retry" | "start_server" | "open_models" | "open_settings";

export interface ErrorInfo {
  title: string;
  body: string;
  learnMore?: string;
  actionHint?: ActionHint;
}

/// Map a raw or friendly error string to actionable, human copy. Keep the
/// branches ordered most-specific first. Anchors match docs/reference.md#troubleshooting.
export function classifyError(e: unknown): ErrorInfo {
  const low = rawMessage(e).toLowerCase();
  if (
    low.includes("server is not running") || low.includes("connection refused") ||
    low.includes("error trying to connect") || low.includes("os error 61") ||
    low.includes("tcp connect error")
  ) {
    return {
      title: "The local server isn't running",
      body: "QuantaMind talks to a local llama-server. Start it from the header, then try again.",
      learnMore: `${DOCS}#server-not-running`,
      actionHint: "start_server",
    };
  }
  if (low.includes("model") && low.includes("not found")) {
    return {
      title: "That model isn't installed",
      body: "Install it from the Models tab, then run your prompt again.",
      learnMore: `${DOCS}#model-not-found`,
      actionHint: "open_models",
    };
  }
  if (low.includes("out of memory") || low.includes("oom") || low.includes("not enough memory")) {
    return {
      title: "Not enough memory for this model",
      body: "Close other apps, or pick a smaller / more-quantized model in the Models tab.",
      learnMore: `${DOCS}#out-of-memory`,
      actionHint: "open_models",
    };
  }
  if (low.includes("timeout") || low.includes("timed out")) {
    return {
      title: "The request timed out",
      body: "Large models can take a while to load. Wait a moment and try again.",
      learnMore: `${DOCS}#timeouts`,
      actionHint: "retry",
    };
  }
  return { title: "Something went wrong", body: rawMessage(e), actionHint: "retry" };
}
