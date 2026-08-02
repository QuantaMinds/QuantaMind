//! Stop a spawned child's whole **process group**, not just the direct child.
//!
//! A server we spawn routinely forks its own children (an MCP server is `npx` →
//! `node`; a customer's agent under `qm certify` is arbitrary). Killing only the pid
//! we hold leaves those grandchildren running, holding ports and memory. The child is
//! made a group leader at spawn time (`process_group(0)` on unix,
//! `CREATE_NEW_PROCESS_GROUP` via `EngineHost::apply_spawn_flags` on Windows) so the
//! whole tree can be signalled by one negative pid.

/// The `kill(1)` argv that signals the process group `pgid` — and ONLY that group.
///
/// The `--` is load-bearing, not style. Without it, procps `kill` (every Linux distro)
/// reads the leading-dash pgid as bundled short options rather than a negative pid, and
/// signals **the caller's own process group** instead of the target. Measured on
/// ubuntu-22.04 (procps, `/bin/kill`), child correctly leading its own group:
///
/// ```text
///   child pid=4192  child pgid=4192  |  caller pgid=4191
///   /bin/kill -TERM -4192   →  caller killed by SIGTERM (exit 143); the group SURVIVED
///   /bin/kill -TERM -- -4192 →  exit 0; the group died; the caller lived
/// ```
///
/// So the old form did the exact opposite of its purpose: it orphaned the server it meant
/// to reap and killed the app that called it. It went unseen because BSD `kill` (macOS)
/// parses the bare form correctly; `--` is accepted by both, so one argv is right on both.
pub(crate) fn group_signal_argv(signal: &str, pgid: u32) -> Vec<String> {
    vec![signal.to_string(), "--".to_string(), format!("-{pgid}")]
}

/// Signal a whole process group, refusing the one target that can never be meant.
///
/// `pgid == 0` is the footgun this guards: `kill -- -0` is `kill(0, sig)`, POSIX for
/// "signal MY OWN process group" — it would kill QuantaMind (and, under a test runner, the
/// harness). A real child always has a non-zero pid, so 0 can only ever arrive from a bug;
/// signalling nothing is the only safe reading.
#[cfg(unix)]
fn signal_group(signal: &str, pgid: u32) {
    if pgid == 0 {
        return;
    }
    let _ = std::process::Command::new("kill")
        .args(group_signal_argv(signal, pgid))
        .stderr(std::process::Stdio::null())
        .status();
}

/// Ask the child's whole process group to exit cleanly. Unix: `kill -TERM -- -<pgid>`
/// (negative pid = the group; see [`group_signal_argv`] for why `--` is required).
/// Windows: `CTRL_BREAK` to the group via `Host`.
///
/// **Windows caveat:** `EngineHost::command` applies `CREATE_NO_WINDOW`, so a child
/// spawned by a GUI launch has no attached console and `GenerateConsoleCtrlEvent`
/// cannot reach it. There, the direct-child `TerminateProcess` after this call is the
/// real stop and grandchildren may survive. A Job Object would close that gap.
pub(crate) fn graceful_stop_group(pid: u32) {
    #[cfg(unix)]
    {
        signal_group("-TERM", pid);
    }
    #[cfg(not(unix))]
    {
        use crate::os::{EngineHost, Host};
        let _ = Host::graceful_stop(pid);
    }
}

/// Hard-kill the child's whole process group. Unix: `kill -KILL -- -<pgid>`.
/// Windows: no-op here — the direct-child `TerminateProcess` (`child.kill()`)
/// after this call is the hard stop, matching the existing sidecar pattern.
pub(crate) fn hard_stop_group(pid: u32) {
    #[cfg(unix)]
    {
        signal_group("-KILL", pid);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE REGRESSION. `kill -TERM -<pgid>` without `--` makes procps `kill` (Linux) read the
    /// leading-dash pgid as bundled short options and signal the CALLER'S OWN process group —
    /// so `McpTransport::kill()` killed QuantaMind and ORPHANED the server it meant to reap,
    /// the exact inverse of its purpose. Measured on ubuntu-22.04: `/bin/kill -TERM -4192`
    /// (child correctly leading group 4192, caller in 4191) killed the caller with SIGTERM
    /// (exit 143) and left the group alive; `/bin/kill -TERM -- -4192` exited 0, killed the
    /// group, and the caller lived. macOS's BSD kill parses the bare form fine, which is why
    /// this only ever bit Linux — and why CI's ubuntu job died mid-suite rather than failing.
    #[test]
    fn a_group_signal_separates_options_so_the_pgid_is_never_read_as_flags() {
        let argv = group_signal_argv("-TERM", 4192);
        assert_eq!(argv, vec!["-TERM", "--", "-4192"], "the `--` separator is load-bearing");
        // Stated as the invariant: the negative pid must come AFTER an end-of-options marker,
        // or `kill` is free to read it as flags and signal us instead.
        let sep = argv.iter().position(|a| a == "--").expect("an end-of-options separator is required");
        let target = argv.iter().position(|a| a == "-4192").expect("the pgid is the target");
        assert!(sep < target, "`--` must precede the negative pid: {argv:?}");
    }

    /// `kill -- -0` is `kill(0, sig)` — POSIX for "signal MY OWN process group". A real child
    /// always has a non-zero pid, so a 0 here is only ever a bug; it must signal nothing rather
    /// than kill the app. Guarded because this is the same class of mistake as the one above.
    #[cfg(unix)]
    #[test]
    fn signalling_group_zero_is_refused_because_it_would_target_our_own_group() {
        // Must not panic, must not signal: we are still alive to make this assertion.
        signal_group("-TERM", 0);
        signal_group("-KILL", 0);
    }

    #[test]
    fn stopping_a_dead_pid_is_harmless() {
        // Reaping is idempotent and racy by nature — the child may already be gone.
        graceful_stop_group(u32::MAX);
        hard_stop_group(u32::MAX);
    }
}
