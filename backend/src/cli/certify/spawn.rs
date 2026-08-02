//! The subprocess actuator: run the customer's agent for one attempt.
//!
//! This is the only file in the certify path that touches `std::process`. It owns
//! the spawn flags, the deadline, the kill escalation, and the stdio drain — the
//! harness engine sees only an `ActuatorReport`.

use crate::cli::certify::command::AgentCommand;
use crate::inference::eval::harness::{ActuatorReport, ActuatorResult, AttemptContext};
use crate::os::proc_group::{graceful_stop_group, hard_stop_group};
use crate::os::{EngineHost, Host};
use crate::redact::redact_path;
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How many stderr lines to keep for a failure report.
const STDERR_TAIL_CAP: usize = 20;
/// How often the deadline loop polls the child. Cheap: `try_wait` is a syscall.
const POLL: Duration = Duration::from_millis(50);
/// How long to wait for the stderr reader to finish after the child exits.
/// Bounded: a grandchild that inherited the pipe can keep it open indefinitely.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Run the agent once. Never returns `Err` — every failure mode is a *reported*
/// outcome, because "the harness broke" and "the agent failed" must stay distinct
/// and both must reach the report.
pub fn run_agent(
    cmd: &AgentCommand,
    ctx: &AttemptContext,
    timeout: Duration,
    kill_grace: Duration,
    echo: bool,
) -> ActuatorReport {
    let started = Instant::now();

    let argv = match cmd.argv_for(ctx) {
        Ok(a) => a,
        Err(e) => return spawn_failed(e.to_string(), started),
    };

    let mut c = Host::command(cmd.program());
    c.args(&argv)
        .current_dir(ctx.workspace)
        .env_clear()
        .envs(cmd.env_for(ctx, None))
        // No stdin. Many agent CLIs block on an interactive confirmation; giving
        // them immediate EOF makes that fail fast instead of burning the timeout.
        .stdin(Stdio::null())
        // Piped AND drained below: an un-drained pipe deadlocks a chatty child
        // once the OS buffer fills.
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Unix: the child leads its own group so teardown reaps its grandchildren too.
    // Windows gets CREATE_NEW_PROCESS_GROUP from `Host::command`'s spawn flags.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }

    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => return spawn_failed(redact_path(&e.to_string()), started),
    };
    let pid = child.id();

    let tail = Arc::new(Mutex::new(Vec::<String>::new()));
    if let Some(out) = child.stdout.take() {
        let _ = drain(out, None, echo, "out");
    }
    // Keep the stderr drain's completion signal: for a fast-exiting child the
    // reader thread may not have processed its last line by the time `try_wait`
    // reports the exit, and a crash is precisely when the tail matters most.
    let stderr_done = child.stderr.take().map(|err| drain(err, Some(Arc::clone(&tail)), echo, "err"));

    // Deadline loop.
    let mut timed_out = false;
    let exit = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {}
            Err(e) => return spawn_failed(redact_path(&e.to_string()), started),
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            // Ask the whole group to leave, then insist.
            graceful_stop_group(pid);
            let deadline = Instant::now() + kill_grace;
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(POLL);
            }
            if !matches!(child.try_wait(), Ok(Some(_))) {
                hard_stop_group(pid);
                let _ = child.kill();
            }
            // Always reap, or we leave a zombie holding the pid.
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(POLL);
    };

    let wall_ms = started.elapsed().as_millis() as u64;

    // Wait for the stderr reader to hit EOF before reading the tail, or the tail
    // is a race: on Linux a child that exits immediately regularly loses it.
    // Bounded, because a surviving grandchild can hold the pipe open — on that
    // path we take whatever arrived rather than hanging the run.
    if let Some(done) = stderr_done {
        let _ = done.recv_timeout(DRAIN_GRACE);
    }
    let stderr_tail = tail.lock().map(|t| t.clone()).unwrap_or_default();

    let result = if timed_out {
        ActuatorResult::TimedOut
    } else {
        // A signal-killed child has no code. Report it as a non-zero exit rather
        // than inventing 0 — it did not succeed.
        ActuatorResult::Exited { code: exit.unwrap_or(-1) }
    };
    ActuatorReport { result, wall_ms, stderr_tail }
}

fn spawn_failed(reason: String, started: Instant) -> ActuatorReport {
    let _ = started;
    ActuatorReport {
        result: ActuatorResult::SpawnFailed { reason },
        // Nothing ran, so nothing was measured — never report 0ms as if it had.
        wall_ms: 0,
        stderr_tail: vec![],
    }
}

/// Drain one pipe on its own thread.
///
/// Every line is redacted before it can reach a terminal or a report — the child's
/// output routinely contains the absolute workspace path we handed it, and rule 7f
/// forbids that in any log. The echo goes to **our stderr**, never stdout, so
/// `--json` stays pipeable to `jq`.
fn drain<R: std::io::Read + Send + 'static>(
    pipe: R,
    tail: Option<Arc<Mutex<Vec<String>>>>,
    echo: bool,
    stream: &'static str,
) -> std::sync::mpsc::Receiver<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            let safe = redact_path(&line);
            if echo {
                eprintln!("[agent:{stream}] {safe}");
            }
            if let Some(t) = &tail {
                if let Ok(mut t) = t.lock() {
                    if t.len() == STDERR_TAIL_CAP {
                        t.remove(0);
                    }
                    t.push(safe);
                }
            }
        }
        // EOF: every line is now in `tail`. A send error just means the reader
        // gave up waiting (the bounded grace elapsed), which is not our problem.
        let _ = tx.send(());
    });
    rx
}
