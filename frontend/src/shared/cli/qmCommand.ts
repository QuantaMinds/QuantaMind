// Build the EXACT `qm` CLI command equivalent to what a Run button will do, from
// the current UI selections — the GCP-console "Equivalent command line" pattern, so
// users learn the CLI by seeing it. Truth-first: every value is verbatim from state;
// where a value genuinely isn't in the UI (a saved-report path) we show an honest
// placeholder + the step to produce it, never a fabricated one.
import type { BackendKind } from "../ipc/models/storage";
import type { InferenceParams } from "../ipc/workspace/prompts";

/// A model placeholder shown when nothing is selected — the command is still
/// copy-able as a template, and `incomplete` lets the UI hint "pick a model".
/// UPPERCASE + shell-safe (GCP-style placeholder) so it never triggers quoting.
const MODEL_PLACEHOLDER = "YOUR_MODEL";

export interface QmCommand {
  command: string;
  /// An honest one-line caveat (e.g. how to produce a value the UI can't supply).
  note?: string;
  /// True when a required value (the model) isn't selected yet.
  incomplete?: boolean;
}

/// Single-quote a value only when it contains a char outside the shell-safe set.
/// Model tags like `qwen2.5:7b` stay bare; names with spaces/parens get quoted.
export function shellQuote(v: string): string {
  if (v === "") return "''";
  if (/^[A-Za-z0-9._:/@-]+$/.test(v)) return v;
  return `'${v.replace(/'/g, `'\\''`)}'`;
}

const flag = (name: string, value: string | number) => `--${name} ${typeof value === "string" ? shellQuote(value) : value}`;

/// All 7 global inference params → CLI flags, emitting ONLY the fields the user set
/// (an unset field is omitted, so the command stays clean and matches the run). The
/// mapping is 1:1 with the backend `InferenceParams` (max_tokens → --num-predict).
export function paramFlags(p: InferenceParams | undefined): string[] {
  if (!p) return [];
  const out: string[] = [];
  if (p.temperature != null) out.push(flag("temperature", p.temperature));
  if (p.top_p != null) out.push(flag("top-p", p.top_p));
  if (p.top_k != null) out.push(flag("top-k", p.top_k));
  if (p.max_tokens != null) out.push(flag("num-predict", p.max_tokens));
  if (p.repeat_penalty != null) out.push(flag("repeat-penalty", p.repeat_penalty));
  if (p.seed != null) out.push(flag("seed", p.seed));
  if (p.num_ctx != null) out.push(flag("num-ctx", p.num_ctx));
  return out;
}

export type RunMode = "native" | "prompt_based" | "both";

/// native/prompt boolean pair → the CLI `--mode` value (the UI has no single field).
export function modeFrom(nativeFc: boolean, promptBased: boolean): RunMode {
  if (nativeFc && promptBased) return "both";
  if (promptBased) return "prompt_based";
  return "native";
}

export interface RunOpts {
  backend: BackendKind;
  model: string | null;
  collection: string;
  isCustom: boolean; // a user collection → `qm test --collection FILE`; else built-in id → `qm run`
  mode: RunMode;
  tier?: string; // omitted when "auto" (CLI then uses the collection's tier)
  thinking: string; // lean | standard | deep
  k: number;
  maxSteps?: number; // UI "Max Steps" → --max-steps (omitted when the tier default)
  decoy?: number; // UI "Decoy Tools" count → --decoy (omitted when off)
  params?: InferenceParams; // global params → --temperature etc.
}

/// Tests page ⇄ `qm run` (built-in id) or `qm test --collection FILE` (custom).
export function buildRunCommand(o: RunOpts): QmCommand {
  const incomplete = !o.model;
  const model = o.model ?? MODEL_PLACEHOLDER;
  const parts = [
    o.isCustom ? "qm test" : "qm run",
    flag("backend", o.backend),
    flag("model", model),
    flag("collection", o.isCustom ? `${o.collection}.json` : o.collection),
    flag("mode", o.mode),
  ];
  if (o.tier) parts.push(flag("tier", o.tier));
  parts.push(flag("thinking", o.thinking), flag("k", o.k));
  if (o.maxSteps != null) parts.push(flag("max-steps", o.maxSteps));
  if (o.decoy != null) parts.push(flag("decoy", o.decoy));
  parts.push(...paramFlags(o.params));
  return {
    command: parts.join(" "),
    incomplete,
    note: o.isCustom ? "custom collection — export it to a .json file first (the app stores it under your data dir)." : undefined,
  };
}

export interface CliffOpts {
  backend: BackendKind;
  model: string | null;
  collection: string;
  maxTokens: number;
  steps: number;
  source: string; // corporate_policy | system_logs | financial_ledger
  native?: boolean; // true → --mode native (default prompt_based)
  params?: InferenceParams; // sampling params (cliff is greedy unless --temperature set)
}

/// Audit page ⇄ `qm cliff`.
export function buildCliffCommand(o: CliffOpts): QmCommand {
  const incomplete = !o.model;
  const model = o.model ?? MODEL_PLACEHOLDER;
  const parts = [
    "qm cliff",
    flag("backend", o.backend),
    flag("model", model),
    flag("collection", o.collection),
    flag("max-tokens", o.maxTokens),
    flag("steps", o.steps),
    flag("source", o.source),
  ];
  if (o.native) parts.push(flag("mode", "native"));
  parts.push(...paramFlags(o.params));
  return { command: parts.join(" "), incomplete };
}

/// Workspace ⇄ `qm prompt` — the free-form generation twin (system+user prompt +
/// params, streamed). The prompt text is read from stdin (kept out of the shown
/// command; a long/multiline prompt as an argv value would be unreadable + unsafe).
export function buildPromptCommand(o: { backend: BackendKind; model: string | null; params?: InferenceParams }): QmCommand {
  const incomplete = !o.model;
  const parts = ["qm prompt", flag("backend", o.backend), flag("model", o.model ?? MODEL_PLACEHOLDER), ...paramFlags(o.params)];
  return {
    command: parts.join(" "),
    incomplete,
    note: "your prompt is read from stdin — pipe it, or add --system '…' --user '…'.",
  };
}

/// Audit "Run History" ⇄ the save + re-assess flow. History accumulates from runs,
/// so there's no single command — this is the runnable chain that builds and reads
/// a history record: `qm run … --save-report run.json && qm report --report run.json`.
export function buildRunHistory(o: {
  backend: BackendKind;
  model: string | null;
  collection: string;
  isCustom: boolean;
  profile: string;
}): QmCommand {
  const model = o.model ?? MODEL_PLACEHOLDER;
  const runCmd = o.isCustom ? "qm test" : "qm run";
  const coll = o.isCustom ? `${o.collection}.json` : o.collection;
  return {
    command: `${runCmd} ${flag("backend", o.backend)} ${flag("model", model)} ${flag("collection", coll)} --save-report run.json && qm report --report run.json ${flag("profile", o.profile)}`,
    incomplete: !o.model,
    note: "each run appends a history record; the second command re-assesses a saved run offline.",
  };
}

/// Agent Report page ⇄ `qm report`. The UI re-assesses the last SAVED run on disk;
/// the CLI needs that run's file, which the UI never exposes — so show an honest
/// `<run>.json` placeholder + the step to produce it.
export function buildReportCommand(o: { profile: string }): QmCommand {
  return {
    command: `qm report --report run.json ${flag("profile", o.profile)}`,
    note: "produce run.json first: qm run … --save-report run.json (the app re-assesses your last saved run).",
  };
}

