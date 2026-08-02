import type { InferenceParams } from "../../../../shared/ipc/workspace/prompts";

export type ParamKey = keyof InferenceParams;

export interface ParamInfo {
  key: ParamKey;
  label: string;
  min: number;
  max: number;
  step: number;
  slider: boolean;
  integer: boolean;
  placeholder: string;
  tooltip: string;
}

/// Display metadata + per-field tooltip copy. Ranges mirror the backend
/// validation in `commands/prompt_options.rs`. `placeholder` shows the
/// effective default when the field is unset.
export const PARAMS: ParamInfo[] = [
  { key: "temperature", label: "Temperature", min: 0, max: 2, step: 0.05, slider: true, integer: false,
    placeholder: "0.7", tooltip: "Randomness. Higher is more creative; 0 is deterministic." },
  { key: "top_p", label: "Top P", min: 0, max: 1, step: 0.05, slider: true, integer: false,
    placeholder: "0.9", tooltip: "Nucleus sampling: only consider tokens within this cumulative probability." },
  { key: "top_k", label: "Top K", min: 0, max: 100, step: 1, slider: true, integer: true,
    placeholder: "40", tooltip: "Sample only from the K most likely tokens. 0 disables the cutoff." },
  { key: "max_tokens", label: "Max tokens", min: 0, max: 4096, step: 1, slider: true, integer: true,
    placeholder: "unlimited", tooltip: "Cap on generated tokens. Empty = model default." },
  { key: "repeat_penalty", label: "Repeat penalty", min: 0, max: 2, step: 0.05, slider: true, integer: false,
    placeholder: "1.1", tooltip: "Discourage repetition. Above 1 penalizes repeated tokens." },
  { key: "seed", label: "Seed", min: 0, max: 0, step: 1, slider: false, integer: true,
    placeholder: "random", tooltip: "Fixed seed gives reproducible output. Empty = random each run." },
  { key: "num_ctx", label: "Context window", min: 0, max: 131072, step: 1024, slider: false, integer: true,
    placeholder: "model default (4096)", tooltip: "Context window (num_ctx). Higher lets the model use more of its window for long prompts, but uses more KV-cache memory. Empty = the model/server default. llama.cpp fixes context at launch, so changing this restarts the server to apply. A remote server's window is fixed by however it was launched." },
];
