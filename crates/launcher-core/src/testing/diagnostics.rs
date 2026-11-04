//! Fakes for the network diagnostics ports.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;

use crate::domain::diagnostics::{
    ConnectionError, ConnectionTimer, Diagnostic, DiagnosticSink, HopProbeError, HopProber,
    ProbeOutcome, ReportHost, ReportShareError, ResolvedTarget, SharedReport,
};
use crate::domain::ports::{Clock, Delay};
use crate::domain::server_list::ServerAddress;

/// A fixed path: probe `ttl` is answered by `path[ttl - 1]` (`None` = a hop
/// that never answers); probes at or past the path's length reach the
/// destination, whose address is the last entry.
#[derive(Debug, Default)]
pub struct FakeHopProber {
    path: Vec<Option<IpAddr>>,
    unavailable: bool,
    destination_silent: bool,
    /// From this ttl on, every `n`th probe is lost.
    losing: Option<(u8, u32)>,
    /// Probes sent per ttl, and every probe in order.
    sent: Mutex<HashMap<u8, u32>>,
    log: Mutex<Vec<u8>>,
}

impl FakeHopProber {
    pub fn path(path: &[Option<&str>]) -> Self {
        Self {
            path: path
                .iter()
                .map(|hop| hop.map(|raw| raw.parse().unwrap()))
                .collect(),
            ..Self::default()
        }
    }

    /// Every probe fails as if the system forbade them.
    pub fn unavailable() -> Self {
        Self {
            unavailable: true,
            ..Self::default()
        }
    }

    /// The destination never answers (it drops ICMP).
    pub fn destination_silent(mut self) -> Self {
        self.destination_silent = true;
        self
    }

    pub fn losing_from(mut self, ttl: u8, every: u32) -> Self {
        self.losing = Some((ttl, every));
        self
    }

    pub fn probes(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    /// The highest ttl probed after the first `rounds` rounds.
    pub fn max_ttl_probed_after_round(&self, rounds: u32) -> u8 {
        let log = self.log.lock().unwrap();
        let mut seen: HashMap<u8, u32> = HashMap::new();
        let mut max = 0;
        for ttl in log.iter() {
            let count = seen.entry(*ttl).or_default();
            *count += 1;
            if *count > rounds {
                max = max.max(*ttl);
            }
        }
        max
    }

    pub fn max_ttl_probed_after_first_round(&self) -> u8 {
        self.max_ttl_probed_after_round(1)
    }
}

impl HopProber for FakeHopProber {
    async fn probe(
        &self,
        _target: IpAddr,
        ttl: u8,
        _timeout: Duration,
    ) -> Result<ProbeOutcome, HopProbeError> {
        if self.unavailable {
            return Err(HopProbeError::Unavailable("not permitted".into()));
        }
        self.log.lock().unwrap().push(ttl);
        let count = {
            let mut sent = self.sent.lock().unwrap();
            let count = sent.entry(ttl).or_default();
            *count += 1;
            *count
        };
        if let Some((from, every)) = self.losing
            && ttl >= from
            && count.is_multiple_of(every)
        {
            return Ok(ProbeOutcome::TimedOut);
        }
        let index = usize::from(ttl) - 1;
        let reached = index + 1 >= self.path.len();
        if reached && self.destination_silent {
            return Ok(ProbeOutcome::TimedOut);
        }
        let from = if reached {
            self.path.last().copied().flatten()
        } else {
            self.path[index]
        };
        Ok(match from {
            Some(from) => ProbeOutcome::Reply {
                from,
                rtt: Duration::from_millis(u64::from(ttl) * 5),
                reached,
            },
            None => ProbeOutcome::TimedOut,
        })
    }
}

/// Resolves to one address after a fixed DNS time and answers connects.
#[derive(Debug)]
pub struct FakeConnectionTimer {
    target: Option<SocketAddr>,
    refusing: bool,
    losing_every: Option<u32>,
    connects: Mutex<u32>,
    resolved: Mutex<u32>,
    rtt: Duration,
}

impl FakeConnectionTimer {
    pub fn resolving(address: &str, rtt_ms: u64) -> Self {
        Self {
            target: Some(address.parse().unwrap()),
            refusing: false,
            losing_every: None,
            connects: Mutex::new(0),
            resolved: Mutex::new(0),
            rtt: Duration::from_millis(rtt_ms),
        }
    }

    pub fn not_found() -> Self {
        Self {
            target: None,
            ..Self::resolving("127.0.0.1:25565", 0)
        }
    }

    pub fn refusing(mut self) -> Self {
        self.refusing = true;
        self
    }

    /// Every `n`th connect times out.
    pub fn losing_every(mut self, n: u32) -> Self {
        self.losing_every = Some(n);
        self
    }

    pub fn resolved(&self) -> u32 {
        *self.resolved.lock().unwrap()
    }
}

impl ConnectionTimer for FakeConnectionTimer {
    async fn resolve(&self, server: &ServerAddress) -> Result<ResolvedTarget, ConnectionError> {
        *self.resolved.lock().unwrap() += 1;
        let address = self
            .target
            .ok_or_else(|| ConnectionError::NameNotFound(server.host().to_owned()))?;
        Ok(ResolvedTarget {
            host: server.host().to_owned(),
            address,
            dns: Duration::from_millis(12),
        })
    }

    async fn connect(
        &self,
        _address: SocketAddr,
        _timeout: Duration,
    ) -> Result<Option<Duration>, ConnectionError> {
        if self.refusing {
            return Err(ConnectionError::Failed("connection refused".into()));
        }
        let mut connects = self.connects.lock().unwrap();
        *connects += 1;
        if self.losing_every.is_some_and(|n| (*connects).is_multiple_of(n)) {
            return Ok(None);
        }
        Ok(Some(self.rtt))
    }
}

/// A clock that moves only when something waits on it, so paced loops run
/// instantly and still see time pass.
#[derive(Debug)]
pub struct SimulatedTime {
    now: Mutex<SystemTime>,
}

impl Default for SimulatedTime {
    fn default() -> Self {
        Self {
            now: Mutex::new(SystemTime::UNIX_EPOCH + Duration::from_secs(1_780_000_000)),
        }
    }
}

impl Clock for SimulatedTime {
    fn now(&self) -> SystemTime {
        *self.now.lock().unwrap()
    }
}

impl Delay for SimulatedTime {
    async fn wait(&self, duration: Duration) {
        *self.now.lock().unwrap() += duration;
    }
}

/// Records every diagnosis update; can cancel a token after N updates.
#[derive(Debug, Default)]
pub struct RecordingDiagnosticSink {
    updates: Mutex<Vec<Diagnostic>>,
    cancel: Mutex<Option<(CancellationToken, usize)>>,
}

impl RecordingDiagnosticSink {
    pub fn count(&self) -> usize {
        self.updates.lock().unwrap().len()
    }

    pub fn last(&self) -> Option<Diagnostic> {
        self.updates.lock().unwrap().last().cloned()
    }

    pub fn cancel_after(&self, token: &CancellationToken, updates: usize) {
        *self.cancel.lock().unwrap() = Some((token.clone(), updates));
    }
}

impl DiagnosticSink for RecordingDiagnosticSink {
    fn update(&self, diagnostic: &Diagnostic) {
        let mut updates = self.updates.lock().unwrap();
        updates.push(diagnostic.clone());
        if let Some((token, after)) = self.cancel.lock().unwrap().as_ref()
            && updates.len() >= *after
        {
            token.cancel();
        }
    }
}

/// Publishes to a fixed link, or fails, and records what it was given.
#[derive(Debug)]
pub struct FakeReportHost {
    answer: Result<SharedReport, ReportShareError>,
    published: Mutex<Vec<String>>,
}

impl FakeReportHost {
    pub fn answering(answer: Result<SharedReport, ReportShareError>) -> Self {
        Self {
            answer,
            published: Mutex::default(),
        }
    }

    pub fn published(&self) -> Vec<String> {
        self.published.lock().unwrap().clone()
    }
}

impl ReportHost for FakeReportHost {
    async fn publish(&self, content: &str) -> Result<SharedReport, ReportShareError> {
        self.published.lock().unwrap().push(content.to_owned());
        self.answer.clone()
    }
}
