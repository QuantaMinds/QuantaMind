import { z } from "zod";

export const EVENT_TOKEN = "prompt-token";
export const EVENT_DONE = "prompt-done";
export const EVENT_CANCELLED = "prompt-cancelled";

export const TokenPayloadSchema = z.object({
  text: z.string(),
});

// Per-token timing entry: text, ms since run start, 1-based cumulative count.
export const TokenTimingSchema = z.object({
  text: z.string(),
  t_ms: z.number().int().nonnegative(),
  n: z.number().int().positive(),
});

// Server-reported final metrics (ms). Every field optional/nullable: a backend
// reports only what it knows; null/absent = "not measured", never 0.
const optMs = z.number().int().nonnegative().nullable().optional();
export const GenerateStatsSchema = z.object({
  prompt_eval_count: optMs,
  prompt_eval_ms: optMs,
  eval_count: optMs,
  eval_ms: optMs,
  load_ms: optMs,
  total_ms: optMs,
  // Prompt tokens served from llama.cpp's prefix cache (reuse) vs recomputed; null for
  // backends without the feature (a remote backend). Drives the prefix-cache readout in the
  // Inspector + Analysis tabs (the backend already sends it on DonePayload.stats).
  cache_n: optMs,
});
export type GenerateStats = z.infer<typeof GenerateStatsSchema>;

export const DonePayloadSchema = z.object({
  ttft_ms: z.number().nullable(),
  tokens_per_sec: z.number().nullable(),
  token_count: z.number().int().nonnegative(),
  timeline: z.array(TokenTimingSchema),
  stats: GenerateStatsSchema.optional(),
});

export const CancelledPayloadSchema = z.object({
  token_count: z.number().int().nonnegative(),
});

export type TokenPayload = z.infer<typeof TokenPayloadSchema>;
export type TokenTiming = z.infer<typeof TokenTimingSchema>;
export type DonePayload = z.infer<typeof DonePayloadSchema>;
export type CancelledPayload = z.infer<typeof CancelledPayloadSchema>;
