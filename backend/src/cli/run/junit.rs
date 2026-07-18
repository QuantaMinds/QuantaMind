//! JUnit XML for `qm run --junit`, so a run shows up in a CI provider's native test
//! panel (GitHub Actions, GitLab, etc.). One `<testsuite>` per run; per measured
//! path (native_fc / prompt_based) a `verdict` testcase, plus one testcase per tier
//! from `by_tier`. A non-Ready verdict, or a tier that didn't pass every task,
//! becomes a `<failure>`. Pure — unit-tested without a live run.
//!
//! Hand-generated (no XML dep — locked stack): every interpolated value is escaped.

use super::RunReport;
use crate::inference::eval::readiness::types::Readiness;

/// Escape the five XML entities so a model/task id or a reason can't break the doc.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Comma-joined `name=count` for the non-zero failure categories, by serializing the
/// tracker (avoids hard-coding its 12 fields — they can't drift out of sync).
fn nonzero_failures<T: serde::Serialize>(f: &T) -> String {
    let Ok(serde_json::Value::Object(m)) = serde_json::to_value(f) else {
        return "no per-task failure detail".into();
    };
    let mut parts: Vec<String> = m
        .iter()
        .filter_map(|(k, v)| v.as_u64().filter(|&n| n > 0).map(|n| format!("{k}={n}")))
        .collect();
    parts.sort();
    if parts.is_empty() {
        "no per-task failure detail".into()
    } else {
        parts.join(", ")
    }
}

/// Render a run report as a JUnit XML document.
pub fn to_junit(r: &RunReport) -> String {
    let mut cases = String::new();
    let mut tests = 0u32;
    let mut failures = 0u32;

    for v in &r.verdicts {
        let path = format!("{:?}", v.verdict.path);
        let cls = format!("qm-run.{}", esc(&path));
        let passk = v.pass_k.map(|p| format!("{p:.2}")).unwrap_or_else(|| "N/A".into());

        // Verdict-level testcase — Ready passes, anything else fails (the panel shows
        // the truth; `--fail-on` separately decides the process exit).
        tests += 1;
        cases.push_str(&format!("    <testcase classname=\"{cls}\" name=\"verdict\">\n"));
        if v.verdict.status != Readiness::Ready {
            failures += 1;
            let reasons: Vec<String> = v.verdict.blocking.iter().chain(v.verdict.conditions.iter()).cloned().collect();
            cases.push_str(&format!(
                "      <failure message=\"{} (pass^k={})\">{}</failure>\n",
                esc(&format!("{:?}", v.verdict.status)),
                esc(&passk),
                esc(&reasons.join("; "))
            ));
        }
        cases.push_str(&format!("      <system-out>pass^k={} runs={}/{}</system-out>\n", esc(&passk), v.passes, v.total_runs));
        cases.push_str("    </testcase>\n");

        // Per-tier breakdown. A tier only FAILS when the verdict itself isn't Ready —
        // so the panel mirrors the exit code (Ready → green, even at k=1 with one task
        // short of perfect; NotReady/Conditional → red, naming the culprit tier). The
        // task tally is always in system-out for context.
        let not_ready = v.verdict.status != Readiness::Ready;
        for t in &v.by_tier {
            tests += 1;
            cases.push_str(&format!("    <testcase classname=\"{cls}\" name=\"tier:{}\">\n", esc(&format!("{:?}", t.tier))));
            if not_ready && t.tasks_passed < t.tasks_total {
                failures += 1;
                cases.push_str(&format!(
                    "      <failure message=\"{}/{} tasks passed\">{}</failure>\n",
                    t.tasks_passed,
                    t.tasks_total,
                    esc(&nonzero_failures(&t.failures))
                ));
            }
            cases.push_str(&format!("      <system-out>{}/{} tasks passed</system-out>\n", t.tasks_passed, t.tasks_total));
            cases.push_str("    </testcase>\n");
        }
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n  <testsuite name=\"qm-run:{}:{}\" tests=\"{tests}\" failures=\"{failures}\" errors=\"0\">\n{cases}  </testsuite>\n</testsuites>\n",
        esc(&r.collection_id),
        esc(&r.model)
    )
}

#[cfg(test)]
#[path = "junit_tests.rs"]
mod tests;
