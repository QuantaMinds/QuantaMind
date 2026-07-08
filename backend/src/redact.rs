//! Path redaction (rule 7f). An absolute path carries the OS username — `/Users/<you>`,
//! `/home/<you>`, `C:\Users\<you>` — so any path that reaches a log, an error body, or (worst)
//! the publish payload leaks machine identity. `redact_path` strips it: the real home dir
//! becomes `~`, and a generic user-home segment has the username masked to `<user>`.
//!
//! Scope note: the agentic transcripts do NOT capture the OS environment — their "env" is the
//! SIMULATED task sandbox (fake file trees / web corpus / UI state defined by the scenario),
//! so there is no username/hostname/env-var there to redact. The genuine machine-identity
//! surfaces are (a) `std::io::Error` strings, which embed full paths, and (b) any path a log
//! prints. This helper is applied at those boundaries and in the publish projection.

/// Replace user-identifying path prefixes so no username survives into a log/error/payload.
pub fn redact_path(s: &str) -> String {
    let mut out = s.to_string();

    // 1) The exact current home dir → `~` (covers every real path this process produces).
    if let Some(home) = dirs::home_dir() {
        let home = home.to_string_lossy();
        if home.len() > 1 {
            out = out.replace(home.as_ref(), "~");
        }
    }

    // 2) Generic user-home segments, so a path built for another user/context still loses the
    //    name: `/Users/alice/x` → `/Users/<user>/x` (and `/home/…`, Windows `\Users\…`).
    out = mask_user_segment(&out, "/Users/", '/');
    out = mask_user_segment(&out, "/home/", '/');
    out = mask_user_segment(&out, "\\Users\\", '\\');
    out
}

/// Replace the path segment right after `marker` (the username) with `<user>`.
fn mask_user_segment(s: &str, marker: &str, sep: char) -> String {
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(idx) = rest.find(marker) {
        let boundary = idx + marker.len();
        result.push_str(&rest[..boundary]);
        let after = &rest[boundary..];
        let end = after.find(sep).unwrap_or(after.len());
        if end == 0 {
            // marker immediately followed by a separator (or end) — no username segment.
            rest = after;
            continue;
        }
        result.push_str("<user>");
        rest = &after[end..];
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
#[path = "redact_tests.rs"]
mod tests;
