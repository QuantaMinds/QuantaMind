use super::types::{ModelVerdict, Readiness};

/// Tier score for the primary sort: Ready > Conditional > NotReady. Integer
/// comparison — no float involved.
fn tier(status: &Readiness) -> u8 {
    match status {
        Readiness::Ready => 2,
        Readiness::Conditional => 1,
        Readiness::NotReady => 0,
    }
}

/// Rank verdicts **best-first** for the recommendation: tier (Ready > Conditional >
/// NotReady), then **effort** (fewer output tokens = better), then **avg_steps**
/// (fewer = better). Float-safe — `f64` is not `Ord`, so we use `total_cmp` and map
/// `None → f64::MAX` so an unmeasured metric **sinks** rather than panicking on a
/// would-be `NaN` or floating above a measured model. (Latency slots before effort
/// once 7.4 wires `ms_per_step`.)
pub fn rank(verdicts: &mut [ModelVerdict]) {
    verdicts.sort_by(|a, b| {
        tier(&b.verdict.status)
            .cmp(&tier(&a.verdict.status)) // higher tier first
            .then_with(|| a.effort.unwrap_or(f64::MAX).total_cmp(&b.effort.unwrap_or(f64::MAX)))
            .then_with(|| a.avg_steps.unwrap_or(f64::MAX).total_cmp(&b.avg_steps.unwrap_or(f64::MAX)))
    });
}

/// The recommended model after ranking — simply the best (first) verdict, or
/// `None` when there are no models.
pub fn recommendation(verdicts: &[ModelVerdict]) -> Option<&ModelVerdict> {
    verdicts.first()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::backend::backend_kind::BackendKind;
    use crate::inference::eval::readiness::types::{AgentPath, CliffStatus, ReadinessVerdict};

    fn verdict(model: &str, status: Readiness, effort: Option<f64>, steps: Option<f64>) -> ModelVerdict {
        ModelVerdict {
            model: model.into(),
            backend: BackendKind::Ollama,
            verdict: ReadinessVerdict {
                status,
                blocking: vec![],
                conditions: vec![],
                path: AgentPath::PromptBased,
                required_tier: Default::default(),
                cleared_tier: None,
            },
            memory: None,
            avg_steps: steps,
            effort,
            pass_k: None,
            quantization: None,
            cliff: CliffStatus::NotProbed,
            by_tier: Vec::new(),
            failures: Default::default(),
            passes: 0,
            total_runs: 0,
        }
    }

    fn order(v: &[ModelVerdict]) -> Vec<&str> {
        v.iter().map(|m| m.model.as_str()).collect()
    }

    #[test]
    fn tier_dominates_ready_then_conditional_then_not_ready() {
        let mut v = vec![
            verdict("z", Readiness::NotReady, Some(10.0), Some(1.0)),
            verdict("r", Readiness::Ready, Some(999.0), Some(99.0)), // worst metrics but Ready → still first
            verdict("c", Readiness::Conditional, Some(1.0), Some(1.0)),
        ];
        rank(&mut v);
        assert_eq!(order(&v), vec!["r", "c", "z"]);
    }

    #[test]
    fn within_a_tier_lower_effort_then_fewer_steps_wins() {
        let mut v = vec![
            verdict("hi", Readiness::Ready, Some(500.0), Some(2.0)),
            verdict("lo", Readiness::Ready, Some(200.0), Some(8.0)), // higher steps but lower effort → first
            verdict("mid", Readiness::Ready, Some(500.0), Some(1.0)), // ties hi on effort, fewer steps
        ];
        rank(&mut v);
        assert_eq!(order(&v), vec!["lo", "mid", "hi"]);
    }

    #[test]
    fn unmeasured_metric_sinks_below_a_measured_one_and_never_panics() {
        let mut v = vec![
            verdict("none", Readiness::Ready, None, None), // unmeasured → f64::MAX → last
            verdict("measured", Readiness::Ready, Some(300.0), Some(3.0)),
        ];
        rank(&mut v);
        assert_eq!(order(&v), vec!["measured", "none"]);
    }

    #[test]
    fn empty_is_safe_and_recommendation_is_the_first() {
        let mut empty: Vec<ModelVerdict> = vec![];
        rank(&mut empty);
        assert!(recommendation(&empty).is_none());
        let v = vec![verdict("only", Readiness::Conditional, Some(1.0), Some(1.0))];
        assert_eq!(recommendation(&v).unwrap().model, "only");
    }
}
