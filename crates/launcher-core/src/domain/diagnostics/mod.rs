//! Network diagnostics: an MTR-style measurement of the path from the player
//! to a server (each hop's loss and latency, the TCP port and the Minecraft
//! ping) and a plain-language verdict on where a problem starts.

mod ports;
mod stats;
mod verdict;
mod zones;

use std::net::IpAddr;
use std::time::Duration;

pub use ports::{
    ConnectionError, ConnectionTimer, DiagnosticSink, HopProbeError, HopProber, ProbeOutcome,
    ReportHost, ReportShareError, ResolvedTarget, SharedReport,
};
pub use stats::{HISTORY_LEN, LatencyStats};
pub use verdict::{
    HopEvidence, IssueKind, JITTER_THRESHOLD_MS, LATENCY_THRESHOLD_MS, LOSS_THRESHOLD, MIN_SAMPLES,
    Verdict, diagnose,
};
pub use zones::{HopZone, PROVIDER_PUBLIC_HOPS, SERVER_HOPS, classify_hops, is_private_address};

use crate::domain::server_list::ServerAddress;

/// The most hops a path may have, like traceroute's default.
pub const MAX_HOPS: u8 = 30;

/// The durations a player can pick; 5 minutes is the default because
/// intermittent loss needs a few hundred samples to show.
pub const DURATION_CHOICES: [Duration; 3] = [
    Duration::from_secs(60),
    Duration::from_secs(300),
    Duration::from_secs(600),
];
pub const DEFAULT_DURATION: Duration = Duration::from_secs(300);

/// One hop of the path.
#[derive(Debug, Clone, PartialEq)]
pub struct Hop {
    /// Its distance from the player (the probe's time-to-live).
    pub ttl: u8,
    /// The address it last answered from; `None` while it never answered.
    pub address: Option<IpAddr>,
    pub zone: HopZone,
    pub stats: LatencyStats,
}

/// Whether hops could be probed on this system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HopProbing {
    Available,
    /// Why not; the diagnosis carries on without hops.
    Unavailable(String),
}

/// Where a diagnosis is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticState {
    Running,
    /// Ran for the whole planned duration.
    Finished,
    /// The player stopped it early.
    Stopped,
}

/// The last status the server answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSnapshot {
    pub online_players: u32,
    pub max_players: u32,
    pub version_name: String,
}

/// The whole diagnosis at one moment: what the UI shows and the report holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub server: ServerAddress,
    /// Where the name resolved to; `None` when it did not.
    pub target: Option<ResolvedTarget>,
    pub tcp: LatencyStats,
    pub minecraft: LatencyStats,
    pub server_status: Option<ServerSnapshot>,
    pub hops: Vec<Hop>,
    pub hop_probing: HopProbing,
    /// Whether the server itself answered the hop probes.
    pub destination_reached: bool,
    pub elapsed: Duration,
    pub planned: Duration,
    pub state: DiagnosticState,
    pub verdict: Verdict,
}

impl Diagnostic {
    /// The stats that stand for "the connection to the server": the server's
    /// own hop when it answers probes, otherwise the TCP connects (many
    /// servers drop ICMP).
    pub fn destination(&self) -> &LatencyStats {
        match self.hops.last() {
            Some(hop) if self.destination_reached => &hop.stats,
            _ => &self.tcp,
        }
    }

    /// Recomputes the zones and the verdict from the current stats.
    pub fn refresh_verdict(&mut self) {
        let addresses: Vec<Option<IpAddr>> = self.hops.iter().map(|hop| hop.address).collect();
        for (hop, zone) in self
            .hops
            .iter_mut()
            .zip(classify_hops(&addresses, self.destination_reached))
        {
            hop.zone = zone;
        }
        if self.target.is_none() {
            self.verdict = Verdict::NameNotFound;
            return;
        }
        let evidence: Vec<HopEvidence<'_>> = self
            .hops
            .iter()
            .map(|hop| HopEvidence {
                zone: hop.zone,
                stats: &hop.stats,
            })
            .collect();
        // An unreachable port outranks whatever ICMP says.
        let verdict = if self.tcp.sent() >= MIN_SAMPLES && !self.tcp.answered() {
            Verdict::Unreachable
        } else {
            diagnose(self.destination(), &evidence)
        };
        self.verdict = verdict;
    }
}
