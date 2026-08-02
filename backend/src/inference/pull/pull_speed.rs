use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Moving-average download-rate tracker over a rolling time window.
/// Push `(now, completed_bytes)` as some servers report progress; ask for
/// `bps(now)` to get the current bytes-per-second estimate.
pub struct SpeedTracker {
    samples: VecDeque<(Instant, u64)>,
    window: Duration,
}

impl SpeedTracker {
    pub fn new(window: Duration) -> Self {
        Self { samples: VecDeque::new(), window }
    }

    pub fn add(&mut self, now: Instant, completed: u64) {
        let cutoff = now.checked_sub(self.window);
        if let Some(cutoff) = cutoff {
            while let Some(&(t, _)) = self.samples.front() {
                if t < cutoff { self.samples.pop_front(); } else { break; }
            }
        }
        self.samples.push_back((now, completed));
    }

    pub fn bps(&self, now: Instant) -> u64 {
        if self.samples.len() < 2 { return 0; }
        // VecDeque guarantees front() and back() are Some when len() >= 1.
        // We just checked len() >= 2 so both are safe by construction.
        let &(t_old, b_old) = self.samples.front().expect("len >= 2");
        let &(_, b_new) = self.samples.back().expect("len >= 2");
        let elapsed = now.saturating_duration_since(t_old).as_secs_f64();
        if elapsed <= 0.0 { return 0; }
        let delta = b_new.saturating_sub(b_old) as f64;
        (delta / elapsed) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_sample_yields_zero_bps() {
        let mut s = SpeedTracker::new(Duration::from_secs(5));
        s.add(Instant::now(), 1000);
        assert_eq!(s.bps(Instant::now()), 0);
    }

    #[test]
    fn one_megabyte_per_second_within_5pct() {
        let mut s = SpeedTracker::new(Duration::from_secs(5));
        let t0 = Instant::now();
        s.add(t0, 0);
        s.add(t0 + Duration::from_secs(1), 1_048_576);
        let bps = s.bps(t0 + Duration::from_secs(1));
        let expected = 1_048_576f64;
        let drift = (bps as f64 - expected).abs() / expected;
        assert!(drift <= 0.05, "bps {bps} drifts {drift} from {expected}");
    }

    #[test]
    fn old_samples_outside_window_are_evicted() {
        let mut s = SpeedTracker::new(Duration::from_secs(5));
        let t0 = Instant::now();
        s.add(t0, 0);
        s.add(t0 + Duration::from_secs(10), 5_000_000);
        assert_eq!(s.samples.len(), 1, "old sample beyond window should be evicted");
    }
}
