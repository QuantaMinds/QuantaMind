use super::*;
use crate::cli::run::RunReport;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::readiness::types::ModelVerdict;

fn report(verdict: serde_json::Value) -> RunReport {
    report_for("qwen2.5:3b", verdict)
}

fn report_for(model: &str, verdict: serde_json::Value) -> RunReport {
    let v: ModelVerdict = serde_json::from_value(verdict).expect("valid ModelVerdict");
    RunReport {
        collection_id: "easy-coding".into(),
        backend: BackendKind::Ollama,
        model: model.into(),
        profile_id: "general-agent".into(),
        verdicts: vec![v],
        costs: None,
    }
}

/// The four FailureTracker fields without `#[serde(default)]` must be present.
fn failures(extra: (&str, u32)) -> serde_json::Value {
    serde_json::json!({
        "infinite_loop_hits": 0, "hallucinated_completions": 0,
        "malformed_json_calls": 0, "schema_unrecovered_calls": 0,
        extra.0: extra.1,
    })
}

#[test]
fn ready_run_is_green_even_when_a_tier_is_short_of_perfect() {
    // Ready at pass^k 0.80 (4/5 tasks) — the panel must be GREEN (mirror the exit code),
    // not show a red tier just because one task didn't pass.
    let xml = to_junit(&report(serde_json::json!({
        "model":"qwen2.5:3b","backend":"ollama",
        "verdict":{"status":"ready","blocking":[],"conditions":[],"path":"prompt_based"},
        "pass_k":0.8,"passes":4,"total_runs":5,
        "by_tier":[{"tier":"easy","tasks_passed":4,"tasks_total":5,"failures":failures(("empty_output_calls",1))}]
    })));
    assert!(xml.starts_with("<?xml version=\"1.0\""));
    assert!(xml.contains("tests=\"2\"")); // verdict + one tier
    assert!(xml.contains("failures=\"0\""), "a Ready run must have zero failures: {xml}");
    assert!(!xml.contains("<failure"));
    assert!(xml.contains("<system-out>4/5 tasks passed</system-out>")); // detail still visible
}

#[test]
fn notready_run_emits_escaped_failures_with_reasons() {
    let xml = to_junit(&report(serde_json::json!({
        "model":"weak","backend":"ollama",
        "verdict":{"status":"not_ready","blocking":["pass^k 0.00 < 0.60 required"],"conditions":[],"path":"prompt_based"},
        "pass_k":0.0,"passes":0,"total_runs":5,
        "by_tier":[{"tier":"easy","tasks_passed":0,"tasks_total":5,"failures":failures(("empty_output_calls",3))}]
    })));
    assert!(xml.contains("failures=\"2\"")); // verdict + tier both fail
    // The `<` in the blocking reason must be escaped, never emitted raw.
    assert!(xml.contains("pass^k 0.00 &lt; 0.60 required"));
    assert!(!xml.contains("0.00 < 0.60"));
    assert!(xml.contains("empty_output_calls=3"));
    assert!(xml.contains("0/5 tasks passed"));
}

#[test]
fn special_chars_in_model_name_are_escaped() {
    let xml = report_and_render("a<b>&\"'");
    assert!(xml.contains("qm-run:easy-coding:a&lt;b&gt;&amp;&quot;&apos;"));
    assert!(!xml.contains("a<b>")); // raw angle brackets never leak into the XML
}

fn report_and_render(model: &str) -> String {
    to_junit(&report_for(
        model,
        serde_json::json!({
            "model":model,"backend":"ollama",
            "verdict":{"status":"ready","blocking":[],"conditions":[],"path":"prompt_based"}
        }),
    ))
}
