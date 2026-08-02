import type { ReactNode } from "react";

/// In-app help for the Latency tab's Test-run view — one source of truth for the copy the
/// InfoButtons show. Every explanation names the metric's PROVENANCE (measured / computed
/// from measured tokens / estimated / claimed), because that ladder is the page's contract.
export const EVAL_RUN_HELP: Record<string, { title: string; body: ReactNode }> = {
  page: {
    title: "Test-run cost view",
    body: (
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <span>
          What the run you launched on the <b>Tests</b> tab actually cost on this machine — per task, live while it
          streams. Works for every source: Built-In, Custom JSON, and MCP collections.
        </span>
        <span>
          Each task gets a card per <b>pass</b>: <b>native FC</b> (the model's tool API) and prompt-based render
          separately, because they are different eval methods — their costs are never blended.
        </span>
        <span>
          Numbers the backend doesn't report show <b>"Not available"</b> — never a guessed value. Workspace-prompt
          per-token timing lives under the <b>Analysis</b> tab.
        </span>
      </div>
    ),
  },
  memory: {
    title: "Memory for this run",
    body: (
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <span>
          <b>Model in memory</b> — llama.cpp: the GGUF's size at launch (it reports no resident split). A remote
          server runs on another machine, so its memory isn't measurable from here.
        </span>
        <span>
          <b>KV cache at this run's peak</b> — the headline: what THIS run's deepest context cost, tokens × the
          canonical bytes-per-token formula. On llama.cpp the token count is measured (cache_n + prompt_n); on a remote backend
          it's an estimate — the label always says which, and at which cache precision (f16 unless the launch says
          q8_0).
        </span>
        <span>
          <b>Server process RSS</b> — diagnostic only: the whole server process's resident RAM at step end (weights +
          leftovers, and GPU-wired buffers may be missing). Never a per-task amount.
        </span>
        <span>
          <b>Fit verdict</b> — model + this run's KV vs your device pool; "Tight" above 70%. A planning estimate, not
          a measured OOM point. On a real OOM the panel names the task and computes the context ceilings that would
          fit instead.
        </span>
        <span>
          <b>Ceiling bars</b> — CAPACITY, not usage: the largest context this machine could hold at each KV precision.
          Your actual usage is the context-window-budget line (tokens — precision changes bytes per token, never the
          count).
        </span>
      </div>
    ),
  },
  taskCard: {
    title: "Per-task metrics",
    body: (
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <span>
          <b>Prefill / Decode</b> — server-reported wall-clock, summed over all Pass^k runs: reading the prompt vs
          generating tokens. The per-step track below shows the same split step by step.
        </span>
        <span>
          <b>Output tokens</b> — tokens generated, all runs. <b>Thinking tokens</b> — on llama.cpp, the reasoning
          channel tokenized with the model's own tokenizer (a measured split); on some backends the server reports ONE
          combined count, shown with <b>"(no split)"</b>.
        </span>
        <span>
          <b>Cache hits</b> — prompt tokens served from the server's prefix cache instead of recomputed. Measured on
          llama.cpp (why later steps are fast); a backend without one reports → "Not available".
        </span>
        <span>
          <b>Peak context</b> — the fullest a single run's window got (reused prefix + recomputed prompt + generated).
          Sizes the KV headline above. Cache-hit totals can exceed it — they accumulate across runs; the peak is one
          moment.
        </span>
        <span>
          <b>Wall</b> — measured clock for the task's whole Pass^k batch, including sandbox/world time between turns.
        </span>
      </div>
    ),
  },
};
