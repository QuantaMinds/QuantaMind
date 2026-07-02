import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

const PropertySchema = z.object({ type: z.string(), description: z.string().optional() });

/// JSON-Schema `parameters` block — the shape developers paste from real tool
/// defs. Zod here is UX only (the backend re-validates every task it runs).
export const ParametersSchema = z.object({
  type: z.literal("object"),
  properties: z.record(z.string(), PropertySchema),
  required: z.array(z.string()).optional(),
});

export const ToolSchemaSchema = z.object({
  name: z.string().min(1),
  description: z.string(),
  parameters: ParametersSchema,
});

const CallSchema = z.object({ name: z.string().min(1), args: z.record(z.string(), z.unknown()) });

export const ExpectedSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("call"), name: z.string().min(1), args: z.record(z.string(), z.unknown()) }),
  z.object({ type: z.literal("parallel"), calls: z.array(CallSchema).min(1) }),
  z.object({ type: z.literal("no_call") }),
]);

// --- Agentic (multi-step) task spec — mirrors backend agentic::spec/sandbox ---

const TaskCheckpointSchema = z.object({ tool: z.string().min(1), args: z.record(z.string(), z.unknown()) });
const MockResponseSchema = z.object({ call: CallSchema, response: z.string() });

/// Mirrors the externally-tagged Rust `EndStateRule`: a unit variant serializes
/// to the bare string `"expect_abstaining_text"`; the tuple variants to
/// `{ "require_sequence": [...] }` (v1, ordered) and `{ "require_all": [...] }`
/// (Phase 9-v2, unordered consume-once — the shape EVERY bundled v2 scenario uses).
export const EndStateRuleSchema = z.union([
  z.object({ require_sequence: z.array(TaskCheckpointSchema).min(1) }),
  z.object({ require_all: z.array(TaskCheckpointSchema).min(1) }),
  z.literal("expect_abstaining_text"),
  // Slice 3 (stateful web-UI): `{ "require_end_state": <target sub-state> }`. MUST be here or
  // `z.object` strips it on the task round-trip → the backend re-receives a different end_state
  // and the state-diff grader breaks (same bug class as the `environment` field below). The
  // target is opaque to the frontend; preserved verbatim.
  z.object({ require_end_state: z.unknown() }),
]);

/// Mirrors the externally-tagged Rust `FaultInjection` (Driver B): a transient
/// error that clears after N attempts, or a fatal one that never clears.
export const FaultInjectionSchema = z.union([
  z.object({ transient_error: z.object({ status_code: z.number().int(), clears_after: z.number().int().nonnegative() }) }),
  z.object({ persistent_error: z.object({ status_code: z.number().int() }) }),
]);
const FaultRuleSchema = z.object({ call: CallSchema, fault: FaultInjectionSchema });

/// The measurable axes that DEFINE a difficulty tier (Phase 9). The Agent Report reads
/// `min_required_steps` + `decoy_tools` to show a tier's real "Task Parameters" instead of
/// the mockup's illustrative ranges. Defaulted so a partial/hand-authored axes still parses.
const DifficultyAxesSchema = z.object({
  min_required_steps: z.number().int().nonnegative().default(0),
  decoy_tools: z.number().int().nonnegative().default(0),
  hidden_prereqs: z.number().int().nonnegative().default(0),
  conflicting_constraints: z.number().int().nonnegative().default(0),
  adversarial_context: z.boolean().default(false),
});
export type DifficultyAxes = z.infer<typeof DifficultyAxesSchema>;

export const AgenticSpecSchema = z.object({
  mocks: z.array(MockResponseSchema),
  end_state: EndStateRuleSchema,
  k: z.number().int().positive().optional(),
  max_steps: z.number().int().positive().optional(),
  /// Driver B lazy-agent traps; omitted/empty for a fault-free task.
  faults: z.array(FaultRuleSchema).optional(),
  /// Driver D semantic-recovery budget; omitted to use the engine default.
  max_recovery: z.number().int().nonnegative().optional(),
  /// Phase 9 difficulty tier (Easy when absent) — inlined enum to avoid a value-import
  /// cycle with readiness.ts (the existing pattern for the tier enum in this file).
  tier: z.enum(["easy", "medium", "hard", "extreme"]).optional(),
  /// Phase 9 difficulty axes; absent for pre-Phase-9 tasks → the Matrix shows "not declared".
  axes: DifficultyAxesSchema.optional(),
  /// Phase 9-v2 opaque fields. The frontend never reads them, but it DOES hand the
  /// parsed tasks straight back to `run_batch_eval` — so they must survive the parse
  /// or a built-in scenario loses its oracle/traps and every run fails. Typed as
  /// `unknown` (permissive: a UX mirror must never reject a valid backend task).
  must_not_call: z.array(z.unknown()).optional(),
  world_state: z.unknown().optional(),
  name_faults: z.array(z.unknown()).optional(),
  generated: z.boolean().optional(),
  /// Backend-derived tool sets (Phase 9-v2): the getter set + the recognized-tool whitelist. MUST
  /// be listed or `z.object()` strips them on the round-trip → (a) the backend re-receives them
  /// empty so decoys ack `{"ok":true}` instead of nudging (a weakened trap), AND (b) the
  /// content-verified `collection_hash` (Slice 4) falsely forks EVERY bundled run (received tasks
  /// no longer equal pristine) → nothing publishable. Opaque to the frontend; preserved verbatim.
  entity_tools: z.array(z.string()).optional(),
  recognized_tools: z.array(z.string()).optional(),
  /// Phase 1: which deterministic environment backs the task ("filesystem" selects the
  /// simulated-filesystem responder). MUST be listed here or `z.object()` strips it on the
  /// task round-trip → the backend re-receives it as `Entity` → the fs env never activates
  /// (read_file acks empty, no visual replay). Opaque to the frontend; preserved verbatim.
  environment: z.string().optional(),
});
export type AgenticSpec = z.infer<typeof AgenticSpecSchema>;

/// Categories that run on the multi-turn agentic engine (mirror of the Rust
/// `is_agentic`). `agent_loop` is the Phase 9-v2 authored-scenario category.
const isAgentic = (category: string) => category === "agentic" || category === "agent_loop";

export const ToolTaskSchema = z
  .object({
    id: z.string().min(1),
    category: z.enum(["single", "parallel", "select", "abstain", "agentic", "agent_loop"]),
    prompt: z.string().min(1),
    tools: z.array(ToolSchemaSchema).min(1),
    expected: ExpectedSchema,
    agentic: AgenticSpecSchema.optional(),
  })
  .superRefine((t, ctx) => {
    if (isAgentic(t.category) && !t.agentic)
      ctx.addIssue({ code: "custom", message: "agentic task requires an agentic spec", path: ["agentic"] });
    if (!isAgentic(t.category) && t.agentic)
      ctx.addIssue({ code: "custom", message: "only agentic tasks may carry an agentic spec", path: ["agentic"] });
  });
export type ToolTask = z.infer<typeof ToolTaskSchema>;

const TaskArraySchema = z.array(ToolTaskSchema);

/// A bundled v2 tiered scenario collection for the picker: id (file stem), short
/// domain, and tier (so the UI groups Easy→Extreme and labels by domain).
export const BuiltinCollectionInfoSchema = z.object({
  id: z.string(),
  label: z.string(),
  domain: z.string(),
  tier: z.enum(["easy", "medium", "hard", "extreme"]),
});
export type BuiltinCollectionInfo = z.infer<typeof BuiltinCollectionInfoSchema>;

/// The bundled v2 tiered scenario collections for the dataset picker. Parsed PER ROW so
/// one malformed collection (e.g. a future scenario with an unknown tier) can't throw the
/// whole array and blank the entire picker — that single-bad-row failure (the c7a697a
/// "built-ins wouldn't load" incident) recurs otherwise. A dropped row is WARNED, never
/// silently swallowed, so a bad scenario stays diagnosable.
export async function listBuiltinCollections(): Promise<BuiltinCollectionInfo[]> {
  const rows = z.array(z.unknown()).parse(await invoke("list_builtin_collections"));
  const out: BuiltinCollectionInfo[] = [];
  for (const row of rows) {
    const parsed = BuiltinCollectionInfoSchema.safeParse(row);
    if (parsed.success) out.push(parsed.data);
    else console.warn("Dropping malformed built-in collection:", (row as { id?: unknown })?.id ?? row, parsed.error.issues);
  }
  return out;
}

/// Tasks for a built-in collection id (a v2 scenario file stem, e.g. "easy-coding").
export async function getBuiltinCollection(id: string): Promise<ToolTask[]> {
  return TaskArraySchema.parse(await invoke("get_builtin_collection", { id }));
}

export async function listCustomCollections(): Promise<string[]> {
  return z.array(z.string()).parse(await invoke("list_custom_collections"));
}

export async function loadCustomCollection(name: string): Promise<ToolTask[]> {
  return TaskArraySchema.parse(await invoke("load_custom_collection", { name }));
}

export async function saveCustomCollection(name: string, tasks: ToolTask[]): Promise<void> {
  await invoke("save_custom_collection", { name, tasks });
}

export async function deleteCustomCollection(name: string): Promise<void> {
  await invoke("delete_custom_collection", { name });
}

/// Import an external `.json` by PATH — Rust reads + caps + validates it; the
/// frontend never loads file contents. Returns the new collection name.
export async function importCustomCollection(sourcePath: string): Promise<string> {
  return z.string().parse(await invoke("import_custom_collection", { sourcePath }));
}

/// Read a picked text file (e.g. a CSV) by PATH — Rust reads + size-caps it; the
/// frontend parses/validates the returned text (used by the CSV importer).
export async function readTextCapped(sourcePath: string): Promise<string> {
  return z.string().parse(await invoke("read_text_capped", { sourcePath }));
}

// --- Collection validation (the offline "answer key works" oracle check) ---

/// Per-task oracle verdict. `reachable`: "yes" (a perfect agent reaches the end state),
/// "no" (the answer key is broken — bad checkpoint tool/args/wildcard), or "not_checkable"
/// (stateful/abstain end-state — confirm with a real run). `discriminating`: whether a
/// do-nothing agent correctly FAILS it (null when N/A). `detail` is the human explanation.
export const TaskValidationSchema = z.object({
  id: z.string(),
  reachable: z.enum(["yes", "no", "not_checkable"]),
  discriminating: z.boolean().nullable(),
  detail: z.string(),
});
export type TaskValidation = z.infer<typeof TaskValidationSchema>;

/// Whole-collection verdict. `ok` iff no structural error AND no task is definitively broken.
/// `structural_error` is set when the schema trust-boundary rejected the collection outright
/// (then `tasks` is empty).
export const CollectionValidationSchema = z.object({
  ok: z.boolean(),
  structural_error: z.string().nullable(),
  tasks: z.array(TaskValidationSchema),
});
export type CollectionValidation = z.infer<typeof CollectionValidationSchema>;

/// Deep-validate a SAVED custom collection offline (structural + oracle answer-key proof).
/// No model, no server — proves each task is solvable and discriminating before a real run.
export async function validateCustomCollection(name: string): Promise<CollectionValidation> {
  return CollectionValidationSchema.parse(await invoke("validate_custom_collection", { name }));
}

/// Dry-run the same deep validation on an external `.json` by PATH BEFORE importing it, so a
/// broken collection is caught (and its bad tasks named) without being written to disk.
export async function validateCollectionFile(sourcePath: string): Promise<CollectionValidation> {
  return CollectionValidationSchema.parse(await invoke("validate_collection_file", { sourcePath }));
}
