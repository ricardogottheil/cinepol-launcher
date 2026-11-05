//! Diagnoses the connection to a server the way MTR does: every second it
//! probes each hop of the path, connects to the server's port and, now and
//! then, pings it like the server list does; then it says in plain terms
//! where a problem starts.

use std::pin::pin;
use std::time::{Duration, SystemTime};

use futures::future::{Either, join_all, select};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::domain::diagnostics::{
    ConnectionTimer, DURATION_CHOICES, Diagnostic, DiagnosticSink, DiagnosticState, Hop,
    HopProbeError, HopProber, HopProbing, HopZone, LatencyStats, MAX_HOPS, ProbeOutcome,
    ServerSnapshot, Verdict,
};
use crate::domain::ports::{Clock, Delay};
use crate::domain::server::ServerStatusProbe;
use crate::domain::server_list::{ServerAddress, ServerAddressError};

/// One round per second, like MTR.
pub const ROUND_INTERVAL: Duration = Duration::from_secs(1);
/// How long one hop probe or connect waits for its answer.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
/// The server list ping runs every this many rounds, to stay light on the server.
pub const STATUS_EVERY: u32 = 5;
/// Rounds after which a path that never reached the server stops probing
/// beyond its last answering hop.
pub const TRIM_AFTER_ROUNDS: u32 = 3;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiagnoseError {
    #[error(transparent)]
    InvalidAddress(#[from] ServerAddressError),
    #[error("{0:?} is not one of the offered durations")]
    InvalidDuration(Duration),
}

/// What a diagnosis needs.
pub struct DiagnoseDeps<'a, H, C, P, D, K> {
    pub hops: &'a H,
    pub connections: &'a C,
    pub status: &'a P,
    pub delay: &'a D,
    pub clock: &'a K,
}

/// Runs for `duration` (one of [`DURATION_CHOICES`]) unless `cancel` stops it
/// first, reporting the diagnosis to `sink` after every round. A stopped
/// diagnosis is still a result: it holds what was measured.
pub async fn diagnose_connection<H, C, P, D, K, S>(
    address: &str,
    duration: Duration,
    deps: &DiagnoseDeps<'_, H, C, P, D, K>,
    sink: &S,
    cancel: &CancellationToken,
) -> Result<Diagnostic, DiagnoseError>
where
    H: HopProber,
    C: ConnectionTimer,
    P: ServerStatusProbe,
    D: Delay,
    K: Clock,
    S: DiagnosticSink,
{
    let server = ServerAddress::parse(address)?;
    if !DURATION_CHOICES.contains(&duration) {
        return Err(DiagnoseError::InvalidDuration(duration));
    }
    let started = deps.clock.now();
    let mut diagnostic = Diagnostic {
        server: server.clone(),
        target: None,
        tcp: LatencyStats::default(),
        minecraft: LatencyStats::default(),
        server_status: None,
        hops: Vec::new(),
        hop_probing: HopProbing::Available,
        destination_reached: false,
        elapsed: Duration::ZERO,
        planned: duration,
        state: DiagnosticState::Running,
        verdict: Verdict::Measuring,
    };

    // A resolver on a dead network can hang for a long time: Stop must not wait for it.
    let resolved = match select(
        pin!(deps.connections.resolve(&server)),
        pin!(cancel.cancelled()),
    )
    .await
    {
        Either::Left((resolved, _)) => resolved,
        Either::Right(_) => return Ok(finish(diagnostic, DiagnosticState::Stopped, sink)),
    };
    match resolved {
        Ok(target) => diagnostic.target = Some(target),
        Err(error) => {
            tracing::info!(%server, %error, "diagnosis: the name did not resolve");
            return Ok(finish(diagnostic, DiagnosticState::Finished, sink));
        }
    }
    sink.update(&diagnostic);

    let mut limit = MAX_HOPS;
    let mut round = 0u32;
    loop {
        if cancel.is_cancelled() {
            return Ok(finish(diagnostic, DiagnosticState::Stopped, sink));
        }
        if elapsed_since(deps.clock, started) >= duration {
            diagnostic.elapsed = duration;
            return Ok(finish(diagnostic, DiagnosticState::Finished, sink));
        }
        let Some(target) = diagnostic.target.clone() else {
            return Ok(finish(diagnostic, DiagnosticState::Finished, sink));
        };
        let probing = diagnostic.hop_probing == HopProbing::Available;
        let hop_probes = join_all(
            (1..=if probing { limit } else { 0 })
                .map(|ttl| deps.hops.probe(target.address.ip(), ttl, PROBE_TIMEOUT)),
        );
        let connect = deps.connections.connect(target.address, PROBE_TIMEOUT);
        let ping = async {
            if round.is_multiple_of(STATUS_EVERY) {
                Some(deps.status.status(&server).await)
            } else {
                None
            }
        };
        let work = async {
            let (hops, connect, ping, ()) =
                futures::join!(hop_probes, connect, ping, deps.delay.wait(ROUND_INTERVAL));
            (hops, connect, ping)
        };
        let (outcomes, connect, ping) = match select(pin!(work), pin!(cancel.cancelled())).await {
            Either::Left((results, _)) => results,
            Either::Right(_) => return Ok(finish(diagnostic, DiagnosticState::Stopped, sink)),
        };

        record_hops(&mut diagnostic, &mut limit, outcomes);
        diagnostic.tcp.record(connect.ok().flatten());
        match ping {
            Some(Ok(status)) => {
                if let Some(latency) = status.latency_ms {
                    diagnostic
                        .minecraft
                        .record(Some(Duration::from_millis(u64::from(latency))));
                }
                diagnostic.server_status = Some(ServerSnapshot {
                    online_players: status.online_players,
                    max_players: status.max_players,
                    version_name: status.version_name,
                });
            }
            Some(Err(_)) => diagnostic.minecraft.record(None),
            None => {}
        }
        round += 1;
        if round >= TRIM_AFTER_ROUNDS && !diagnostic.destination_reached {
            let last_answering = diagnostic
                .hops
                .iter()
                .rposition(|hop| hop.stats.answered())
                .map_or(0, |index| index + 1);
            // Keep one silent hop after the last answering one. The limit
            // follows the path both ways: when that silent hop starts to
            // answer (a router that rate-limited ICMP at first), the next
            // one is probed too.
            let keep = (last_answering + 1).min(usize::from(MAX_HOPS));
            diagnostic.hops.truncate(keep);
            limit = u8::try_from(keep).unwrap_or(MAX_HOPS);
        }
        diagnostic.elapsed = elapsed_since(deps.clock, started).min(duration);
        diagnostic.refresh_verdict();
        sink.update(&diagnostic);
    }
}

/// Folds one round of hop probes (`outcomes[i]` probed time-to-live `i + 1`).
fn record_hops(
    diagnostic: &mut Diagnostic,
    limit: &mut u8,
    outcomes: Vec<Result<ProbeOutcome, HopProbeError>>,
) {
    if let Some(Err(HopProbeError::Unavailable(reason))) = outcomes
        .iter()
        .find(|outcome| matches!(outcome, Err(HopProbeError::Unavailable(_))))
    {
        tracing::info!(%reason, "diagnosis: hop probing is not available");
        diagnostic.hop_probing = HopProbing::Unavailable(reason.clone());
        diagnostic.hops.clear();
        diagnostic.destination_reached = false;
        return;
    }
    for (index, outcome) in outcomes.into_iter().enumerate() {
        let Ok(ttl) = u8::try_from(index + 1) else {
            break;
        };
        if ttl > *limit {
            break;
        }
        if diagnostic.hops.len() < usize::from(ttl) {
            diagnostic.hops.push(Hop {
                ttl,
                address: None,
                zone: HopZone::Home,
                stats: LatencyStats::default(),
            });
        }
        let hop = &mut diagnostic.hops[index];
        match outcome {
            Ok(ProbeOutcome::Reply { from, rtt, reached }) => {
                hop.address = Some(from);
                hop.stats.record(Some(rtt));
                if reached {
                    diagnostic.destination_reached = true;
                    *limit = ttl;
                    diagnostic.hops.truncate(usize::from(ttl));
                    break;
                }
            }
            Ok(ProbeOutcome::TimedOut) | Err(_) => hop.stats.record(None),
        }
    }
}

fn elapsed_since<K: Clock>(clock: &K, started: SystemTime) -> Duration {
    clock.now().duration_since(started).unwrap_or_default()
}

fn finish<S: DiagnosticSink>(
    mut diagnostic: Diagnostic,
    state: DiagnosticState,
    sink: &S,
) -> Diagnostic {
    diagnostic.state = state;
    diagnostic.refresh_verdict();
    sink.update(&diagnostic);
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagnostics::IssueKind;
    use crate::domain::server::{ServerStatus, ServerStatusError};
    use crate::testing::{
        FakeConnectionTimer, FakeHopProber, FakeServerStatusProbe, RecordingDiagnosticSink,
        SimulatedTime,
    };

    const CINEPOL_PATH: [Option<&str>; 8] = [
        Some("192.168.68.1"),
        Some("192.168.1.254"),
        Some("190.248.0.245"),
        Some("200.24.33.104"),
        None,
        Some("200.24.33.90"),
        Some("206.41.108.129"),
        Some("103.195.101.252"),
    ];

    fn online() -> FakeServerStatusProbe {
        FakeServerStatusProbe::answering(Ok(ServerStatus {
            online_players: 11,
            max_players: 100,
            version_name: "Velocity-CTD 1.21.11-26.3".into(),
            protocol: 777,
            motd: "CINEPOL".into(),
            latency_ms: Some(48),
        }))
    }

    struct World {
        hops: FakeHopProber,
        connections: FakeConnectionTimer,
        status: FakeServerStatusProbe,
        time: SimulatedTime,
        sink: RecordingDiagnosticSink,
    }

    impl World {
        fn new(hops: FakeHopProber) -> Self {
            Self {
                hops,
                connections: FakeConnectionTimer::resolving("103.195.101.252:25565", 41),
                status: online(),
                time: SimulatedTime::default(),
                sink: RecordingDiagnosticSink::default(),
            }
        }

        async fn run(&self, duration: Duration, cancel: &CancellationToken) -> Diagnostic {
            let deps = DiagnoseDeps {
                hops: &self.hops,
                connections: &self.connections,
                status: &self.status,
                delay: &self.time,
                clock: &self.time,
            };
            diagnose_connection("mc.policarpiers.com", duration, &deps, &self.sink, cancel)
                .await
                .unwrap()
        }
    }

    const ONE_MINUTE: Duration = Duration::from_secs(60);

    #[tokio::test]
    async fn measures_every_hop_for_the_whole_duration() {
        let world = World::new(FakeHopProber::path(&CINEPOL_PATH));
        let diagnostic = world.run(ONE_MINUTE, &CancellationToken::new()).await;

        assert_eq!(diagnostic.state, DiagnosticState::Finished);
        assert_eq!(diagnostic.elapsed, ONE_MINUTE);
        assert_eq!(diagnostic.tcp.sent(), 60);
        assert_eq!(diagnostic.minecraft.sent(), 60 / STATUS_EVERY);
        assert_eq!(diagnostic.minecraft.average_ms(), Some(48.0));
        assert!(diagnostic.destination_reached);
        assert_eq!(diagnostic.hops.len(), 8);
        let ttls: Vec<u8> = diagnostic.hops.iter().map(|hop| hop.ttl).collect();
        assert_eq!(ttls, (1..=8).collect::<Vec<_>>());
        assert!(diagnostic.hops.iter().all(|hop| hop.stats.sent() == 60));
        assert_eq!(diagnostic.hops[4].address, None);
        assert_eq!(diagnostic.hops[0].zone, HopZone::Home);
        assert_eq!(diagnostic.hops[7].zone, HopZone::Server);
        assert_eq!(diagnostic.verdict, Verdict::Healthy);
        assert_eq!(
            diagnostic.server_status.unwrap().version_name,
            "Velocity-CTD 1.21.11-26.3"
        );
        // Once reached, nothing past the server is probed again.
        assert_eq!(world.hops.max_ttl_probed_after_first_round(), 8);
        // One update after resolving, one per round, one at the end.
        assert_eq!(world.sink.count(), 62);
    }

    #[tokio::test]
    async fn locates_loss_that_starts_at_the_provider() {
        let world = World::new(FakeHopProber::path(&CINEPOL_PATH).losing_from(3, 10));
        let diagnostic = world.run(ONE_MINUTE, &CancellationToken::new()).await;
        assert_eq!(
            diagnostic.verdict,
            Verdict::Issue {
                kind: IssueKind::Loss,
                zone: Some(HopZone::Provider)
            }
        );
    }

    #[tokio::test]
    async fn stopping_keeps_what_was_measured() {
        let world = World::new(FakeHopProber::path(&CINEPOL_PATH));
        let cancel = CancellationToken::new();
        world.sink.cancel_after(&cancel, 20);
        let diagnostic = world.run(Duration::from_secs(300), &cancel).await;
        assert_eq!(diagnostic.state, DiagnosticState::Stopped);
        assert_eq!(diagnostic.elapsed, Duration::from_secs(19));
        assert_eq!(diagnostic.planned, Duration::from_secs(300));
        assert_eq!(diagnostic.tcp.sent(), 19);
        assert_eq!(world.sink.last().unwrap().state, DiagnosticState::Stopped);
    }

    #[tokio::test]
    async fn without_hop_probing_the_port_still_decides() {
        let mut world = World::new(FakeHopProber::unavailable());
        world.connections = world.connections.losing_every(5);
        let diagnostic = world.run(ONE_MINUTE, &CancellationToken::new()).await;
        assert!(matches!(diagnostic.hop_probing, HopProbing::Unavailable(_)));
        assert!(diagnostic.hops.is_empty());
        assert_eq!(diagnostic.tcp.loss_percent(), 20.0);
        assert_eq!(
            diagnostic.verdict,
            Verdict::Issue {
                kind: IssueKind::Loss,
                zone: None
            }
        );
    }

    #[tokio::test]
    async fn a_server_that_drops_icmp_is_judged_by_its_port_and_trims_silent_hops() {
        let mut path = CINEPOL_PATH.to_vec();
        path[7] = None; // The server never answers ICMP.
        let world = World::new(FakeHopProber::path(&path).destination_silent());
        let diagnostic = world.run(ONE_MINUTE, &CancellationToken::new()).await;
        assert!(!diagnostic.destination_reached);
        // Seven hops up to the last answering one, plus one silent hop.
        assert_eq!(diagnostic.hops.len(), 8);
        assert_eq!(world.hops.max_ttl_probed_after_round(TRIM_AFTER_ROUNDS), 8);
        assert_eq!(diagnostic.verdict, Verdict::Healthy);
    }

    #[tokio::test]
    async fn a_router_that_answers_late_is_still_measured() {
        let mut path = CINEPOL_PATH.to_vec();
        path[7] = None; // The server never answers ICMP.
        // Hop 7 is silent for the first three rounds, then answers.
        let world = World::new(FakeHopProber::path(&path).destination_silent().late(7, 3));
        let diagnostic = world.run(ONE_MINUTE, &CancellationToken::new()).await;
        assert_eq!(diagnostic.hops.len(), 8);
        assert!(diagnostic.hops[6].stats.answered());
        assert_eq!(world.hops.max_ttl_probed_after_round(TRIM_AFTER_ROUNDS), 8);
    }

    #[tokio::test]
    async fn stopping_while_the_name_resolves_ends_at_once() {
        let mut world = World::new(FakeHopProber::path(&CINEPOL_PATH));
        world.connections = FakeConnectionTimer::resolving("103.195.101.252:25565", 10).hanging();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let diagnostic =
            tokio::time::timeout(Duration::from_secs(5), world.run(ONE_MINUTE, &cancel))
                .await
                .expect("Stop must not wait for the resolver");
        assert_eq!(diagnostic.state, DiagnosticState::Stopped);
        assert_eq!(world.hops.probes(), 0);
    }

    #[tokio::test]
    async fn a_closed_port_is_unreachable() {
        let mut world = World::new(FakeHopProber::path(&CINEPOL_PATH));
        world.connections = world.connections.refusing();
        world.status =
            FakeServerStatusProbe::answering(Err(ServerStatusError::Unreachable("refused".into())));
        let diagnostic = world.run(ONE_MINUTE, &CancellationToken::new()).await;
        assert_eq!(diagnostic.verdict, Verdict::Unreachable);
        assert_eq!(diagnostic.minecraft.loss_percent(), 100.0);
    }

    #[tokio::test]
    async fn a_name_that_does_not_resolve_ends_at_once() {
        let mut world = World::new(FakeHopProber::path(&CINEPOL_PATH));
        world.connections = FakeConnectionTimer::not_found();
        let diagnostic = world.run(ONE_MINUTE, &CancellationToken::new()).await;
        assert_eq!(diagnostic.verdict, Verdict::NameNotFound);
        assert_eq!(diagnostic.state, DiagnosticState::Finished);
        assert_eq!(world.hops.probes(), 0);
        assert_eq!(world.sink.count(), 1);
    }

    #[tokio::test]
    async fn refuses_bad_input_before_touching_the_network() {
        let world = World::new(FakeHopProber::path(&CINEPOL_PATH));
        let deps = DiagnoseDeps {
            hops: &world.hops,
            connections: &world.connections,
            status: &world.status,
            delay: &world.time,
            clock: &world.time,
        };
        let cancel = CancellationToken::new();
        assert!(matches!(
            diagnose_connection("bad host!", ONE_MINUTE, &deps, &world.sink, &cancel).await,
            Err(DiagnoseError::InvalidAddress(_))
        ));
        assert_eq!(
            diagnose_connection(
                "mc.policarpiers.com",
                Duration::from_secs(7),
                &deps,
                &world.sink,
                &cancel
            )
            .await,
            Err(DiagnoseError::InvalidDuration(Duration::from_secs(7)))
        );
        assert_eq!(world.connections.resolved(), 0);
        assert_eq!(world.sink.count(), 0);
    }
}
