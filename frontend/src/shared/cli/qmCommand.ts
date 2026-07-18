// Build the EXACT `qm` CLI command equivalent to what a Run button will do, from
// the current UI selections — the GCP-console "Equivalent command line" pattern, so
// users learn the CLI by seeing it. Truth-first: every value is verbatim from state;
// where a value genuinely isn't in the UI (a saved-report path) we show an honest
// placeholder + the step to produce it, never a fabricated one.
import type { BackendKind } from "../ipc/models/storage";

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
}

/// Audit page ⇄ `qm cliff`.
export function buildCliffCommand(o: CliffOpts): QmCommand {
  const incomplete = !o.model;
  const model = o.model ?? MODEL_PLACEHOLDER;
  const command = [
    "qm cliff",
    flag("backend", o.backend),
    flag("model", model),
    flag("collection", o.collection),
    flag("max-tokens", o.maxTokens),
    flag("steps", o.steps),
    flag("source", o.source),
  ].join(" ");
  return { command, incomplete };
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

/// Workspace ⇄ (no eval command) — a pointer to the headless eval path instead of a
/// fabricated command, since a single interactive prompt has no `qm` equivalent.
export function workspacePointerCommand(model: string | null): string {
  return `qm run --model ${shellQuote(model ?? MODEL_PLACEHOLDER)} --collection easy-coding`;
}
