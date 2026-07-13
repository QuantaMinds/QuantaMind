# Tier Task Schema (v2) — extends the Phase 6/9 contract

Top-level:
  name, domain, tier (Easy|Medium|Hard|Extreme), pass_k, axes{...},
  generated (bool — true if Hard/Extreme procedural), tasks[]

axes: min_required_steps, decoy_tools, hidden_prereqs, conflicting_constraints,
      adversarial_context (bool), region_variance (bool — the "every state different" chaos)

task:
  id, category, max_steps, max_recovery, prompt,
  world_state{}        — the ground-truth the oracle knows; the model must DISCOVER it via tools
  tools[]              — {name, params, returns_entity?, returns_fields?}
  decoy_tools[]        — plausible-but-wrong {name, params}
  expected_calls[]     — type-tagged: {type:call,name,args} | {type:parallel,calls[]} | {type:none}
  must_not_call[]      — names or {name,args} that auto-fail end-state if invoked (the trap)
  faults[]             — {on_call, type:transient|persistent, status_code, clears_after}
  trap                 — {note} explaining why naive/one-rule-fits-all fails
  rubric               — {success_definition, partial_credit:false} (end-state only)

Hybrid generation (Hard/Extreme): `generated:true` + a worked example instance in tasks[],
plus `generator{template_id, seed_from[], randomize[]}` describing what instantiate() varies.

## world_state key rule: reachable or reserved

Every top-level `world_state` key is one of exactly two things:

- **An entity the model is meant to fetch.** It must be reachable: named
  whole-word in the prompt, referenced inside another entity's blob, an
  expected-call arg value, or equal to a tool name (the no-arg/computation
  fallback). `derive_response` returns the WHOLE sub-object for the first arg
  value (or tool name) matching such a key.
- **Oracle/meta data listed in `world_state::RESERVED`** (`calc`, `threshold`,
  `ground_truth`, `outcome`, `rule`/`rules`, `expected_tax`, `real_bug`,
  `sponsor_note`, `fake_coa`, … — the const in `world_state.rs` is the full
  list). Reserved keys are never resolvable by any call (the responder acks)
  and never alpha-renamed by `instantiate()`.

There is no third category: an unreserved key that no intended path reaches is
an answer-key leak — any call whose arg happens to equal the key string would
be handed the whole oracle blob. CI enforces both directions:
`no_unfetched_world_state_key_is_resolvable_by_a_getter` (a leakable key must
be reserved) and `every_expected_getter_call_resolves_to_real_world_state_data`
(reserving a key a real getter needs turns red). When authoring, put scoring
data (`outcome`, per-entity verdicts, expected values) under a reserved key —
never under a fetchable one.

## The rest of the authoring contract

The full user-facing version lives at `docs/reference.md#agentic-authoring-contract`;
the mechanics that matter when writing a scenario file:

- **Getters vs actions:** `returns_entity` absent/true = getter (surfaces the
  entity blob); `false` = action (acks, never echoes data — answer-leniency).
  The reporter tool (a `text` param) is exempt from the getter-data guard: its
  ack IS its response.
- **Field-scoped getters (`returns_fields`):** a getter may declare a field
  subset — `get_service {returns_fields:["class"]}` surfaces ONLY `class` of the
  resolved blob; `check_sessions {returns_fields:["active_sessions"]}` surfaces
  ONLY `active_sessions`. This models a real API where one resource is read
  through different endpoints returning disjoint fields, so a model can't read
  one fact from the other's call — making BOTH calls genuinely required instead
  of interchangeable. Absent → the whole blob (back-compat). A requested field
  missing from an entity yields `{}` (honest absence, never fabricated); the
  integrity guard rejects a `returns_fields` naming no world_state field (a typo
  would silently surface `{}`).
- **Decoys:** merged into the presented tool list at transpile but excluded from
  `entity_tools`/`recognized_tools` — a decoy call gets the unknown-tool nudge,
  never data. Pair with `must_not_call` (bare name, or `{name,args}` to trap a
  real tool on the wrong entity → `ForbiddenCall`).
- **Faults:** keyed by tool NAME with a global counter — `clears_after: N` means
  the N+1-th call on that tool succeeds. The oracle satisfiability test retries
  through transient faults; a `persistent` fault on a required tool is a dead
  end it rejects.
- **Prompts + generated tasks:** name every root entity id in the prompt —
  `instantiate()` alpha-renames digit-bearing ids across prompt + world_state +
  checkpoints + must_not_call, anchored on those mentions. Glob checkpoint args
  for tolerant strings (`"decision": "*FULL*"`).
- **Answer-token grounding:** every glob literal an expected ACTION/REPORTER
  checkpoint demands (`decision:"*no filing*"`, `content:"*quantize*"`) must be
  teachable — present in the prompt, a tool name, or data an EARLIER expected
  call surfaces. Grading on wording the model can never read manufactures
  false-negative fails (a capable model phrases the same conclusion its own
  way). Matching is separator-tolerant (`work-product` grounds "work product").
  Fix by teaching the word in the blob the play reads, never by loosening the
  checkpoint. Severity: WARNING for custom collections (heuristic — the author
  judges; save/import never blocks on it), zero-tolerance for bundled (CI).
- **Realistic-path aliases:** a document blob read via a globbed getter path
  (`read_file{path:"*test_x*"}`) should exist under BOTH its short key and a
  realistic path key (`tests/test_x.py`), with the real path surfaced in the
  runner blob (`failing_test_file`) — otherwise the checkpoint advances on a
  realistic arg while the responder returns `not found` (data/checkpoint
  asymmetry).
- **Shared enforcement:** all of the above is checked by
  `oracle::semantic_findings` — the same function behind the `scenarios.rs` CI
  guards, `evals::save` (custom collections hard-block at write time on
  Error-severity findings), and the import dry-run / Validate button
  (`validate_collection_deep`, per-task `semantic` errors +
  `semantic_warnings`).
