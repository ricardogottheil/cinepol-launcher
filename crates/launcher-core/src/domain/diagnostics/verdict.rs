use super::stats::LatencyStats;
use super::zones::HopZone;

/// Probes to the server before the diagnosis says anything.
pub const MIN_SAMPLES: u32 = 10;
/// Loss to the server, in percent, that counts as a problem.
pub const LOSS_THRESHOLD: f64 = 2.0;
/// Average round trip to the server, in milliseconds, that counts as high.
pub const LATENCY_THRESHOLD_MS: f64 = 150.0;
/// Jitter to the server, in milliseconds, that counts as unstable.
pub const JITTER_THRESHOLD_MS: f64 = 30.0;

/// What is wrong with the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// Packets get lost.
    Loss,
    /// The round trip is long.
    Latency,
    /// The round trip varies a lot (lag spikes).
    Jitter,
}

/// The plain-language conclusion of a diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Not enough samples yet.
    Measuring,
    /// The connection to the server looks fine.
    Healthy,
    /// The server's name could not be resolved.
    NameNotFound,
    /// The server's port never accepted a connection.
    Unreachable,
    /// A problem, and the zone where it starts when the hops tell.
    Issue {
        kind: IssueKind,
        zone: Option<HopZone>,
    },
}

/// One hop as the verdict sees it.
#[derive(Debug, Clone, Copy)]
pub struct HopEvidence<'a> {
    pub zone: HopZone,
    pub stats: &'a LatencyStats,
}

/// Concludes from the stats to the server (`destination`: the server's own
/// hop when it answers ICMP, otherwise the TCP connects) and the hops.
///
/// Loss outranks latency, latency outranks jitter. Loss or delay at a hop
/// counts only when it carries on to every later answering hop: routers
/// often deprioritize answering probes, which shows as loss or delay at
/// that hop alone and is harmless.
pub fn diagnose(destination: &LatencyStats, hops: &[HopEvidence<'_>]) -> Verdict {
    if destination.sent() < MIN_SAMPLES {
        return Verdict::Measuring;
    }
    if !destination.answered() {
        return Verdict::Unreachable;
    }
    let answering: Vec<HopEvidence<'_>> = hops
        .iter()
        .copied()
        .filter(|hop| hop.stats.answered())
        .collect();

    let loss = destination.loss_percent();
    if loss >= LOSS_THRESHOLD {
        let floor = (loss / 2.0).max(LOSS_THRESHOLD / 2.0);
        let zone = answering
            .iter()
            .enumerate()
            .find(|(index, hop)| {
                hop.stats.loss_percent() >= floor
                    && answering[index + 1..]
                        .iter()
                        .all(|later| later.stats.loss_percent() >= floor)
            })
            .map(|(_, hop)| hop.zone);
        return Verdict::Issue {
            kind: IssueKind::Loss,
            zone,
        };
    }
    if destination
        .average_ms()
        .is_some_and(|average| average >= LATENCY_THRESHOLD_MS)
    {
        return Verdict::Issue {
            kind: IssueKind::Latency,
            zone: largest_lasting_rise(&answering, LatencyStats::average_ms),
        };
    }
    if destination
        .jitter_ms()
        .is_some_and(|jitter| jitter >= JITTER_THRESHOLD_MS)
    {
        return Verdict::Issue {
            kind: IssueKind::Jitter,
            zone: largest_lasting_rise(&answering, LatencyStats::jitter_ms),
        };
    }
    Verdict::Healthy
}

/// The zone of the hop where `metric` rises the most and stays up (every
/// later hop keeps at least half of that rise).
fn largest_lasting_rise(
    hops: &[HopEvidence<'_>],
    metric: fn(&LatencyStats) -> Option<f64>,
) -> Option<HopZone> {
    let values: Vec<f64> = hops
        .iter()
        .map(|hop| metric(hop.stats).unwrap_or(0.0))
        .collect();
    let mut best: Option<(f64, HopZone)> = None;
    let mut before = 0.0;
    for (index, hop) in hops.iter().enumerate() {
        let rise = values[index] - before;
        let kept = values[index] - rise / 2.0;
        let lasts = values[index + 1..].iter().all(|later| *later >= kept);
        if rise > 0.0 && lasts && best.is_none_or(|(top, _)| rise > top) {
            best = Some((rise, hop.zone));
        }
        before = values[index];
    }
    best.map(|(_, zone)| zone)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// `sent` probes, `lost` of them lost, the rest answered after `ms`.
    fn stats(sent: u32, lost: u32, ms: u64) -> LatencyStats {
        let mut stats = LatencyStats::default();
        for index in 0..sent {
            stats.record((index >= lost).then(|| Duration::from_millis(ms)));
        }
        stats
    }

    /// Answers alternating between `low` and `high` milliseconds.
    fn shaky(sent: u32, low: u64, high: u64) -> LatencyStats {
        let mut stats = LatencyStats::default();
        for index in 0..sent {
            let ms = if index.is_multiple_of(2) { low } else { high };
            stats.record(Some(Duration::from_millis(ms)));
        }
        stats
    }

    fn hops(list: &[(HopZone, LatencyStats)]) -> Vec<HopEvidence<'_>> {
        list.iter()
            .map(|(zone, stats)| HopEvidence { zone: *zone, stats })
            .collect()
    }

    #[test]
    fn waits_for_enough_samples() {
        assert_eq!(diagnose(&stats(9, 0, 40), &[]), Verdict::Measuring);
    }

    #[test]
    fn a_port_that_never_answers_is_unreachable() {
        assert_eq!(diagnose(&stats(20, 20, 0), &[]), Verdict::Unreachable);
    }

    #[test]
    fn a_clean_path_is_healthy() {
        let path = [
            (HopZone::Home, stats(100, 0, 2)),
            (HopZone::Internet, stats(100, 0, 40)),
            (HopZone::Server, stats(100, 0, 41)),
        ];
        assert_eq!(diagnose(&path[2].1, &hops(&path)), Verdict::Healthy);
    }

    #[test]
    fn loss_starting_at_home_points_at_the_home_network() {
        let path = [
            (HopZone::Home, stats(100, 10, 2)),
            (HopZone::Provider, stats(100, 9, 8)),
            (HopZone::Server, stats(100, 10, 41)),
        ];
        assert_eq!(
            diagnose(&path[2].1, &hops(&path)),
            Verdict::Issue {
                kind: IssueKind::Loss,
                zone: Some(HopZone::Home)
            }
        );
    }

    #[test]
    fn loss_at_one_router_only_is_ignored() {
        // Like hop 5 of the reference screenshot: 25 % there, none after.
        let path = [
            (HopZone::Provider, stats(100, 25, 4)),
            (HopZone::Internet, stats(100, 0, 40)),
            (HopZone::Server, stats(100, 0, 41)),
        ];
        assert_eq!(diagnose(&path[2].1, &hops(&path)), Verdict::Healthy);
    }

    #[test]
    fn loss_that_starts_near_the_server_points_at_the_server() {
        let path = [
            (HopZone::Home, stats(100, 0, 2)),
            (HopZone::Provider, stats(100, 30, 9)), // rate limiting only
            (HopZone::Internet, stats(100, 0, 40)),
            (HopZone::Server, stats(100, 6, 41)),
            (HopZone::Server, stats(100, 5, 42)),
        ];
        assert_eq!(
            diagnose(&path[4].1, &hops(&path)),
            Verdict::Issue {
                kind: IssueKind::Loss,
                zone: Some(HopZone::Server)
            }
        );
    }

    #[test]
    fn silent_hops_do_not_hide_where_the_loss_starts() {
        let path = [
            (HopZone::Provider, stats(100, 8, 9)),
            (HopZone::Internet, stats(100, 100, 0)),
            (HopZone::Server, stats(100, 8, 41)),
        ];
        assert_eq!(
            diagnose(&path[2].1, &hops(&path)),
            Verdict::Issue {
                kind: IssueKind::Loss,
                zone: Some(HopZone::Provider)
            }
        );
    }

    #[test]
    fn without_hops_the_zone_is_unknown() {
        assert_eq!(
            diagnose(&stats(50, 10, 40), &[]),
            Verdict::Issue {
                kind: IssueKind::Loss,
                zone: None
            }
        );
    }

    #[test]
    fn high_latency_points_at_the_lasting_jump() {
        let path = [
            (HopZone::Home, stats(100, 0, 2)),
            (HopZone::Provider, stats(100, 0, 300)), // a slow-answering router
            (HopZone::Provider, stats(100, 0, 10)),
            (HopZone::Internet, stats(100, 0, 190)),
            (HopZone::Server, stats(100, 0, 195)),
        ];
        assert_eq!(
            diagnose(&path[4].1, &hops(&path)),
            Verdict::Issue {
                kind: IssueKind::Latency,
                zone: Some(HopZone::Internet)
            }
        );
    }

    #[test]
    fn jitter_from_the_first_hop_points_at_the_home_network() {
        let path = [
            (HopZone::Home, shaky(100, 2, 80)),
            (HopZone::Internet, shaky(100, 40, 120)),
            (HopZone::Server, shaky(100, 41, 121)),
        ];
        assert_eq!(
            diagnose(&path[2].1, &hops(&path)),
            Verdict::Issue {
                kind: IssueKind::Jitter,
                zone: Some(HopZone::Home)
            }
        );
    }
}
