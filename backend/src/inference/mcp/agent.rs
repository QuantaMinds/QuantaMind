//! The production multi-turn agent loop — the first path where a real model
//! drives a real side-effecting tool *in a loop*. Its three safety rails are the
//! hard precondition for going live (see the phase-9 doc): a **hard iteration
//! cap**, the **result-must-inject** guarantee (every result — including a
//! denial — enters the next turn's context, so the model can't spin forever
//! un-informed), and the **deny-by-default gate** ([`crate::inference::mcp::gate`]).
//!
//! The loop is generic over a `TurnDriver` (the model) and a `ToolExecutor` (the
//! tools) so the rails are unit-tested with fakes before any live model runs.

use crate::errors::AppResult;
use crate::inference::mcp::bridge::{execute_call, ToolExecution};
use crate::inference::mcp::gate::Decision;
use crate::inference::ollama::ollama_chat::NativeToolCall;
use crate::mcp::client::McpClient;

/// One model turn: given the running transcript, the assistant's text + the tool
/// calls it wants to make.
#[allow(async_fn_in_trait)]
pub trait TurnDriver {
    async fn turn(&mut self, transcript: &str) -> AppResult<TurnOutput>;
}

pub struct TurnOutput {
    pub text: String,
    pub calls: Vec<NativeToolCall>,
}

/// Executes one approved tool call (against MCP).
#[allow(async_fn_in_trait)]
pub trait ToolExecutor {
    async fn execute(&self, call: &NativeToolCall) -> AppResult<ToolExecution>;
}

/// The real executor: runs approved calls against a connected MCP client.
pub struct McpExecutor<'a> {
    client: &'a McpClient,
}
impl<'a> McpExecutor<'a> {
    pub fn new(client: &'a McpClient) -> Self {
        McpExecutor { client }
    }
}
impl ToolExecutor for McpExecutor<'_> {
    async fn execute(&self, call: &NativeToolCall) -> AppResult<ToolExecution> {
        execute_call(self.client, call).await
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The model made no tool call — it answered / yielded.
    ModelYielded,
    /// The hard iteration cap was hit (a possible runaway — stopped honestly).
    HitCap,
}

pub struct LoopOutcome {
    pub steps: usize,
    pub executions: Vec<ToolExecution>,
    pub denied: usize,
    pub stopped: StopReason,
    pub final_text: String,
}

/// Run the loop. `gate` decides each call (deny-by-default is baked into the
/// gate the caller passes). A denied call is NOT executed; its denial is still
/// injected so the model learns it was refused.
pub async fn run_loop<D, E, G>(
    driver: &mut D,
    executor: &E,
    gate: G,
    max_steps: usize,
) -> AppResult<LoopOutcome>
where
    D: TurnDriver,
    E: ToolExecutor,
    G: Fn(&NativeToolCall) -> Decision,
{
    let mut transcript = String::new();
    let mut executions = Vec::new();
    let mut denied = 0usize;

    for step in 0..max_steps {
        let out = driver.turn(&transcript).await?;
        if !out.text.is_empty() {
            transcript.push_str(&format!("\nassistant: {}", out.text));
        }
        if out.calls.is_empty() {
            return Ok(LoopOutcome {
                steps: step + 1,
                executions,
                denied,
                stopped: StopReason::ModelYielded,
                final_text: out.text,
            });
        }
        for call in &out.calls {
            let exec = match gate(call) {
                Decision::Approve => executor.execute(call).await?,
                Decision::Deny => {
                    denied += 1;
                    ToolExecution {
                        tool: call.name.clone(),
                        is_error: true,
                        text: "[denied by approval gate]".into(),
                    }
                }
            };
            // RESULT-MUST-INJECT: every result (incl. a denial) enters the next
            // turn's context.
            transcript.push_str(&format!("\ntool[{}]: {}", exec.tool, exec.text));
            executions.push(exec);
        }
    }

    Ok(LoopOutcome {
        steps: max_steps,
        executions,
        denied,
        stopped: StopReason::HitCap,
        final_text: String::new(),
    })
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
