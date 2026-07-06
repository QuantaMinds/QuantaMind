# Tier Task Schema (v2) — extends the Phase 6/9 contract

Top-level:
  name, domain, tier (Easy|Medium|Hard|Extreme), pass_k, axes{...},
  generated (bool — true if Hard/Extreme procedural), tasks[]

axes: min_required_steps, decoy_tools, hidden_prereqs, conflicting_constraints,
      adversarial_context (bool), region_variance (bool — the "every state different" chaos)

task:
  id, category, max_steps, max_recovery, prompt,
  world_state{}        — the ground-truth the oracle knows; the model must DISCOVER it via tools
  tools[]              — {name, params}
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
