//! The safety-rail proofs (hard precondition for going live): the cap stops a
//! runaway, a denied call is never executed, and every result is injected.

use super::*;
use crate::inference::mcp::gate::Decision;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

fn a_call(name: &str) -> NativeToolCall {
    NativeToolCall { name: name.into(), args: json!({}) }
}

/// A scripted model: yields the given (text, calls) per turn, repeating the last
/// forever (to exercise the cap). Records the transcript it last saw.
struct ScriptDriver {
    script: Vec<(String, Vec<NativeToolCall>)>,
    idx: usize,
    last_transcript: String,
}
impl ScriptDriver {
    fn new(script: Vec<(&str, Vec<NativeToolCall>)>) -> Self {
        ScriptDriver {
            script: script.into_iter().map(|(t, c)| (t.to_string(), c)).collect(),
            idx: 0,
            last_transcript: String::new(),
        }
    }
}
impl TurnDriver for ScriptDriver {
    async fn turn(&mut self, transcript: &str) -> AppResult<TurnOutput> {
        self.last_transcript = transcript.to_string();
        let i = self.idx.min(self.script.len() - 1);
        self.idx += 1;
        let (text, calls) = self.script[i].clone();
        Ok(TurnOutput { text, calls })
    }
}

/// Executor that records how many times it ran and returns a fixed marker.
struct CountingExecutor {
    ran: AtomicUsize,
    reply: String,
}
impl ToolExecutor for CountingExecutor {
    async fn execute(&self, call: &NativeToolCall) -> AppResult<ToolExecution> {
        self.ran.fetch_add(1, Ordering::Relaxed);
        Ok(ToolExecution { tool: call.name.clone(), is_error: false, text: self.reply.clone() })
    }
}

#[tokio::test]
async fn hard_cap_stops_a_runaway_that_never_yields() {
    // Model calls a tool every turn, forever.
    let mut d = ScriptDriver::new(vec![("", vec![a_call("loop_tool")])]);
    let e = CountingExecutor { ran: AtomicUsize::new(0), reply: "ok".into() };
    let out = run_loop(&mut d, &e, |_| Decision::Approve, 3).await.unwrap();
    assert_eq!(out.stopped, StopReason::HitCap);
    assert_eq!(out.steps, 3, "stopped exactly at the cap");
    assert_eq!(e.ran.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn model_yielding_ends_the_loop_early() {
    let mut d = ScriptDriver::new(vec![("all done".into(), vec![])]);
    let e = CountingExecutor { ran: AtomicUsize::new(0), reply: "x".into() };
    let out = run_loop(&mut d, &e, |_| Decision::Approve, 5).await.unwrap();
    assert_eq!(out.stopped, StopReason::ModelYielded);
    assert_eq!(out.steps, 1);
    assert!(out.executions.is_empty());
    assert_eq!(out.final_text, "all done");
}

#[tokio::test]
async fn a_denied_call_is_never_executed_but_is_injected() {
    // Turn 1: a write call (denied). Turn 2: yield.
    let mut d = ScriptDriver::new(vec![
        ("", vec![a_call("write_file")]),
        ("stopping", vec![]),
    ]);
    let e = CountingExecutor { ran: AtomicUsize::new(0), reply: "SHOULD_NOT_RUN".into() };
    let out = run_loop(&mut d, &e, |_| Decision::Deny, 5).await.unwrap();

    assert_eq!(e.ran.load(Ordering::Relaxed), 0, "a denied call must NOT reach the executor");
    assert_eq!(out.denied, 1);
    assert_eq!(out.executions.len(), 1);
    assert!(out.executions[0].is_error);
    assert!(out.executions[0].text.contains("denied"));
    // The denial was injected so the model saw it on turn 2.
    assert!(d.last_transcript.contains("denied by approval gate"));
}

#[tokio::test]
async fn every_tool_result_is_injected_into_the_next_turn() {
    let mut d = ScriptDriver::new(vec![
        ("", vec![a_call("read_text_file")]),
        ("done", vec![]),
    ]);
    let e = CountingExecutor { ran: AtomicUsize::new(0), reply: "RESULT_BLUEBIRD".into() };
    let out = run_loop(&mut d, &e, |_| Decision::Approve, 5).await.unwrap();

    assert_eq!(e.ran.load(Ordering::Relaxed), 1);
    // Turn 2's transcript must carry turn 1's real result.
    assert!(
        d.last_transcript.contains("RESULT_BLUEBIRD"),
        "the tool result must be injected: {}",
        d.last_transcript
    );
    assert_eq!(out.stopped, StopReason::ModelYielded);
}
