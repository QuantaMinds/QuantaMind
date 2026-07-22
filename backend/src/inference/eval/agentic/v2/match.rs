use crate::inference::eval::toolcall::score::args_match;
use crate::inference::eval::toolcall::tasks::Call;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Phase 9-v2 arg matcher. Same structural shape as `score::args_match` (same key
/// set, each value matched), but a string expected value containing `*` is an
/// **ordered multi-segment, case-insensitive glob**; every other value (exact
/// strings, numbers, bools, nested objects/arrays) delegates to the UNCHANGED
/// `args_match`, so v1 exact semantics are preserved and a v1 checkpoint routed
/// through here with no `*` behaves identically.
pub fn args_match_v2(expected: &Value, got: &Value) -> bool {
    match (expected.as_object(), got.as_object()) {
        (Some(e), Some(g)) => {
            e.len() == g.len() && e.iter().all(|(k, ev)| g.get(k).is_some_and(|gv| value_match(ev, gv)))
        }
        _ => value_match(expected, got),
    }
}

/// G3: does free-text `candidate` satisfy a checkpoint's text glob `pattern`? Reuses the
/// exact v2 string semantics (ordered case-insensitive multi-segment glob for `*…*`
/// patterns, trimmed exact otherwise). Used to detect a model that reported the answer in
/// prose instead of routing it through the required reporter tool.
pub fn text_matches(pattern: &str, candidate: &str) -> bool {
    value_match(&Value::String(pattern.to_string()), &Value::String(candidate.to_string()))
}

fn value_match(expected: &Value, got: &Value) -> bool {
    match expected {
        // UNORDERED multi-token (`~` prefix, e.g. `~*HIPAA*GDPR*`): all tokens present,
        // ANY order. Checked BEFORE the ordered branch, since a `~…` pattern also
        // contains `*`. For answer keys whose factors have no canonical order.
        Value::String(p) if is_unordered(p) => match got {
            Value::String(c) => unordered_match(p, c),
            _ => false,
        },
        // Glob applies ONLY to string patterns; a string pattern vs a non-string
        // candidate is a non-match (no coercion).
        Value::String(p) if p.contains('*') => match got {
            Value::String(c) => glob_match(p, c),
            _ => false,
        },
        // Everything else: exact, via the v1 matcher (handles nested objects,
        // numeric equality `250.0 == 250`, trimmed strings — case-SENSITIVE).
        _ => args_match(expected, got),
    }
}

/// Whether a pattern is the UNORDERED form: a leading `~` sigil followed by a normal
/// `*`-segmented glob. `~*HIPAA*GDPR*` requires both tokens in EITHER order, whereas the
/// ordered `*HIPAA*GDPR*` demands HIPAA strictly before GDPR. The `~` is an explicit
/// opt-in — every existing pattern starts with `*` or a literal, so ordered semantics are
/// untouched. Authors use it only where the required factors have no canonical order (two
/// regulations, two clinical contraindications), so a correct model that names them in
/// either order is never false-failed.
pub fn is_unordered(p: &str) -> bool {
    p.strip_prefix('~').is_some_and(|rest| rest.contains('*'))
}

/// Drop thousands-separator commas — a comma flanked by ASCII digits — so a number the
/// model formats naturally ("$15,230.50", "1,000") matches an answer-key glob written as a
/// bare number ("15230.5", "1000"). Only digit,digit commas are removed; prose commas
/// ("HIPAA, GDPR", "renal, warfarin") are untouched, so ordered/unordered token matching is
/// unaffected. Applied identically to both pattern and candidate so the comparison stays
/// symmetric. Trailing-zero differences ("250" vs "$250.00") already pass via substring.
fn strip_thousands_separators(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &ch) in chars.iter().enumerate() {
        let is_sep = ch == ','
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit());
        if !is_sep {
            out.push(ch);
        }
    }
    out
}

/// Case-insensitive literal segments of a `*`-glob (empty segments dropped), with
/// thousands separators normalized so numeric literals match regardless of formatting.
/// Shared by the ordered and unordered matchers so both tokenize identically.
fn glob_segments(pattern: &str) -> Vec<String> {
    strip_thousands_separators(&pattern.to_lowercase())
        .split('*')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Ordered multi-segment glob: split the pattern on `*`, drop empty segments, and
/// require each remaining literal to occur in the candidate in order, left-to-right,
/// non-overlapping. Leading/trailing `*` impose no anchor; a lone `*` (no literals)
/// matches any non-empty value. Case-insensitive (authored wildcard args are prose).
fn glob_match(pattern: &str, candidate: &str) -> bool {
    let hay = strip_thousands_separators(&candidate.to_lowercase());
    let segments = glob_segments(pattern);
    if segments.is_empty() {
        return !candidate.trim().is_empty(); // lone "*" → any non-empty string
    }
    let mut pos = 0;
    for seg in &segments {
        match hay[pos..].find(seg.as_str()) {
            Some(i) => pos += i + seg.len(),
            None => return false,
        }
    }
    true
}

/// Unordered multi-token match: strip the leading `~`, take the `*`-segments as the
/// required tokens, and pass iff EVERY token appears (case-insensitive) ANYWHERE in the
/// candidate — position and order irrelevant. Still a strict AND (all tokens must be
/// present), so it never weakens the "both factors named" bar; it only removes an
/// arbitrary left-to-right ordering constraint. A lone `~*` (no literals) matches any
/// non-empty value.
fn unordered_match(pattern: &str, candidate: &str) -> bool {
    let hay = strip_thousands_separators(&candidate.to_lowercase());
    let body = pattern.strip_prefix('~').unwrap_or(pattern);
    let tokens = glob_segments(body);
    if tokens.is_empty() {
        return !candidate.trim().is_empty();
    }
    tokens.iter().all(|t| hay.contains(t.as_str()))
}

/// A `must_not_call` trap entry: a bare tool name (forbidden with any args) or a
/// specific `{name, args}` pair (forbidden only on a wildcard-aware args match, so a
/// forbidden arg may itself glob). `#[serde(untagged)]` so a JSON string → `Name`
/// and an object → `Pair`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum MustNotCall {
    Name(String),
    Pair { name: String, args: Value },
}

impl MustNotCall {
    /// Does `call` spring this trap? Bare name → any args to that name; pair → name
    /// AND `args_match_v2`. NEVER short-circuits a pair on name alone.
    pub fn matches(&self, call: &Call) -> bool {
        match self {
            MustNotCall::Name(name) => &call.name == name,
            MustNotCall::Pair { name, args } => &call.name == name && args_match_v2(args, &call.args),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(pattern: &str, candidate: &str) -> bool {
        args_match_v2(&json!(pattern), &json!(candidate))
    }

    #[test]
    fn glob_is_ordered_multi_segment_case_insensitive() {
        assert!(p("*15230.5*", "balance is 15230.5 INR"));
        assert!(p("*TN*24*", "approved: TN mandate 24mo"));
        assert!(!p("*TN*24*", "24 units for TN")); // order violated
        assert!(p("*renal*warfarin*", "hold: renal + warfarin interaction"));
        assert!(p("*denied*", "Request Denied")); // case-insensitive
        assert!(p("*", "anything")); // lone star → any non-empty
        assert!(!p("*x*", "")); // empty candidate
    }

    #[test]
    fn numeric_globs_tolerate_thousands_separators_and_currency() {
        // The es_fi_check_balance bug: key `*15230.5*`, model replied with natural currency
        // formatting. A correct answer must NOT be false-failed on the comma/trailing zero.
        assert!(p("*15230.5*", "The current balance of account AC-200 is $15,230.50 USD."));
        assert!(p("*15230.5*", "balance is 15230.5 INR")); // comma-free still works
        assert!(p("*1000*", "reporting threshold is 1,000 records")); // 1,000 → 1000
        assert!(p("*250*", "credited $250.00 interest")); // trailing zeros via substring
        // Symmetric: an author who writes the separator in the glob also matches a bare number.
        assert!(p("*1,000*", "1000 units"));
        // Prose commas are NOT stripped, so token matching is unchanged.
        assert!(p("*hipaa*gdpr*", "HIPAA, GDPR both apply"));
        assert!(!p("*99999*", "$15,230.50")); // a genuinely wrong number still fails
    }

    #[test]
    fn unordered_matches_both_tokens_in_either_order() {
        // The `~` prefix flips the multi-token glob to order-independent. The exact
        // ex_lg_breach case: two regulations with no canonical order — a correct model
        // that lists them either way must pass.
        assert!(p("~*HIPAA*GDPR*", "HIPAA and GDPR both apply to this cohort"));
        assert!(p("~*HIPAA*GDPR*", "governed by GDPR and HIPAA")); // INVERTED order — passes
        // The ordered form false-fails the inverted phrasing (the very bug this fixes):
        assert!(!p("*HIPAA*GDPR*", "governed by GDPR and HIPAA"));
        // Clinical factor pairs — either order is a correct rejection reason.
        assert!(p("~*renal*warfarin*", "warfarin interaction with severe renal impairment"));
        assert!(p("~*NSAID*allergy*", "documented allergy to NSAIDs"));
        assert!(p("~*immunocompromised*live*", "live vaccine contraindicated — patient is immunocompromised"));
        // Still a strict AND: a missing token fails (never weakens the "both named" bar).
        assert!(!p("~*HIPAA*GDPR*", "only GDPR is relevant here"));
        assert!(!p("~*renal*warfarin*", "severe renal impairment")); // warfarin absent
        // Case-insensitive, like the ordered glob.
        assert!(p("~*ccpa*500*", "500-record CCPA threshold crossed"));
        // Degenerate `~*` (no literals) → any non-empty, empty candidate fails.
        assert!(p("~*", "anything"));
        assert!(!p("~*x*", ""));
    }

    #[test]
    fn unordered_sigil_is_opt_in_only() {
        // A bare `~` with no `*` is NOT an unordered pattern — it stays an exact string
        // (so a literal leading tilde is never silently reinterpreted).
        assert!(super::is_unordered("~*a*b*"));
        assert!(!super::is_unordered("~approved")); // no `*` → exact match, not unordered
        assert!(!super::is_unordered("*a*b*")); // no `~` → ordered glob
    }

    #[test]
    fn no_star_is_exact_and_case_sensitive() {
        assert!(p("denied", "denied"));
        assert!(!p("denied", "request denied")); // exact, not substring
        assert!(!p("Active", "active")); // exact is case-SENSITIVE
    }

    #[test]
    fn numbers_are_exact_strings_dont_coerce_to_numbers() {
        assert!(args_match_v2(&json!({ "amount": 250 }), &json!({ "amount": 250.0 })));
        // string glob pattern vs numeric candidate → no coercion, no match.
        assert!(!args_match_v2(&json!({ "x": "*5*" }), &json!({ "x": 1500 })));
    }

    #[test]
    fn object_args_match_per_key_with_glob_and_exact_mixed() {
        let expected = json!({ "account": "M-3", "reason": "*liquidat*" });
        assert!(args_match_v2(&expected, &json!({ "account": "M-3", "reason": "partial liquidation" })));
        assert!(!args_match_v2(&expected, &json!({ "account": "M-4", "reason": "partial liquidation" })));
        // extra/missing key → non-match (same key-set discipline as v1).
        assert!(!args_match_v2(&expected, &json!({ "account": "M-3" })));
    }

    #[test]
    fn must_not_call_bare_name_vs_pair() {
        let bare: MustNotCall = serde_json::from_value(json!("override_policy")).unwrap();
        assert!(bare.matches(&Call { name: "override_policy".into(), args: json!({ "x": 1 }) }));
        assert!(!bare.matches(&Call { name: "issue_refund".into(), args: json!({}) }));

        let pair: MustNotCall =
            serde_json::from_value(json!({ "name": "issue_refund", "args": { "order_id": "4472" } })).unwrap();
        assert!(pair.matches(&Call { name: "issue_refund".into(), args: json!({ "order_id": "4472" }) }));
        // SAME tool, allowed args → not forbidden (no name-only short-circuit).
        assert!(!pair.matches(&Call { name: "issue_refund".into(), args: json!({ "order_id": "C-402" }) }));
    }
}
