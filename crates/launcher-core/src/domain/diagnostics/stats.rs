use std::collections::VecDeque;
use std::time::Duration;

/// How many recent samples a hop keeps for its history sparkline.
pub const HISTORY_LEN: usize = 60;

/// Round-trip statistics of one measured target (a hop, the TCP port, the
/// Minecraft ping), in the shape MTR reports them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LatencyStats {
    sent: u32,
    received: u32,
    last: Option<f64>,
    best: Option<f64>,
    worst: Option<f64>,
    sum: f64,
    /// The previous answered sample, for jitter.
    previous: Option<f64>,
    jitter_sum: f64,
    jitter_count: u32,
    history: VecDeque<Option<f64>>,
}

impl LatencyStats {
    /// Records one probe: its round trip, or `None` when it got no answer.
    pub fn record(&mut self, sample: Option<Duration>) {
        self.sent += 1;
        let millis = sample.map(|rtt| rtt.as_secs_f64() * 1000.0);
        if let Some(ms) = millis {
            self.received += 1;
            self.last = Some(ms);
            self.best = Some(self.best.map_or(ms, |best| best.min(ms)));
            self.worst = Some(self.worst.map_or(ms, |worst| worst.max(ms)));
            self.sum += ms;
            if let Some(previous) = self.previous {
                self.jitter_sum += (ms - previous).abs();
                self.jitter_count += 1;
            }
            self.previous = Some(ms);
        }
        if self.history.len() == HISTORY_LEN {
            self.history.pop_front();
        }
        self.history.push_back(millis);
    }

    pub fn sent(&self) -> u32 {
        self.sent
    }

    pub fn received(&self) -> u32 {
        self.received
    }

    /// Lost probes in percent; 0 before anything was sent.
    pub fn loss_percent(&self) -> f64 {
        if self.sent == 0 {
            return 0.0;
        }
        f64::from(self.sent - self.received) * 100.0 / f64::from(self.sent)
    }

    /// Whether this target ever answered.
    pub fn answered(&self) -> bool {
        self.received > 0
    }

    pub fn last_ms(&self) -> Option<f64> {
        self.last
    }

    pub fn best_ms(&self) -> Option<f64> {
        self.best
    }

    pub fn worst_ms(&self) -> Option<f64> {
        self.worst
    }

    pub fn average_ms(&self) -> Option<f64> {
        (self.received > 0).then(|| self.sum / f64::from(self.received))
    }

    /// The mean difference between consecutive answers.
    pub fn jitter_ms(&self) -> Option<f64> {
        (self.jitter_count > 0).then(|| self.jitter_sum / f64::from(self.jitter_count))
    }

    /// The latest samples, oldest first (`None` = lost).
    pub fn history(&self) -> Vec<Option<f64>> {
        self.history.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Option<Duration> {
        Some(Duration::from_millis(value))
    }

    #[test]
    fn empty_stats_report_nothing() {
        let stats = LatencyStats::default();
        assert_eq!(stats.loss_percent(), 0.0);
        assert_eq!(stats.average_ms(), None);
        assert_eq!(stats.jitter_ms(), None);
        assert!(!stats.answered());
    }

    #[test]
    fn computes_loss_last_best_worst_average_and_jitter() {
        let mut stats = LatencyStats::default();
        for sample in [ms(10), None, ms(30), ms(20)] {
            stats.record(sample);
        }
        assert_eq!((stats.sent(), stats.received()), (4, 3));
        assert_eq!(stats.loss_percent(), 25.0);
        assert_eq!(stats.last_ms(), Some(20.0));
        assert_eq!(stats.best_ms(), Some(10.0));
        assert_eq!(stats.worst_ms(), Some(30.0));
        assert_eq!(stats.average_ms(), Some(20.0));
        // |30 - 10| and |20 - 30|: the lost probe does not break the chain.
        assert_eq!(stats.jitter_ms(), Some(15.0));
        assert_eq!(stats.history(), [Some(10.0), None, Some(30.0), Some(20.0)]);
    }

    #[test]
    fn the_history_keeps_the_latest_samples_only() {
        let mut stats = LatencyStats::default();
        for value in 0..(HISTORY_LEN as u64 + 5) {
            stats.record(ms(value));
        }
        let history = stats.history();
        assert_eq!(history.len(), HISTORY_LEN);
        assert_eq!(history[0], Some(5.0));
        assert_eq!(stats.sent(), HISTORY_LEN as u32 + 5);
    }
}
