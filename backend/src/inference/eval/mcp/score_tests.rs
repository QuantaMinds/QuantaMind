use super::*;

#[test]
fn strict_pass_k_treats_a_lucky_pass_as_not_ready() {
    let all = McpScore { k: 3, passes: 3, failures: vec![] };
    assert!(all.is_ready());
    assert!((all.pass_rate() - 1.0).abs() < 1e-9);

    // 1/3 — one lucky pass would fool a single-shot test, but not pass^k.
    let flaky = McpScore { k: 3, passes: 1, failures: vec![vec!["x".into()], vec!["y".into()]] };
    assert!(!flaky.is_ready(), "reliability requires ALL k to pass");
    assert!((flaky.pass_rate() - 1.0 / 3.0).abs() < 1e-9);

    let empty = McpScore { k: 0, passes: 0, failures: vec![] };
    assert!(!empty.is_ready());
    assert_eq!(empty.pass_rate(), 0.0);
}
