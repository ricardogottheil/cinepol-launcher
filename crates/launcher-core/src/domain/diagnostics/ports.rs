use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use thiserror::Error;

use super::Diagnostic;
use crate::domain::server_list::ServerAddress;

/// What one hop probe got back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// A router answered (`reached: false`, its time-to-live expired there)
    /// or the target itself did (`reached: true`).
    Reply {
        from: IpAddr,
        rtt: Duration,
        reached: bool,
    },
    /// Nothing came back in time.
    TimedOut,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum HopProbeError {
    /// The system does not let the launcher send probes (no permission, no
    /// ICMP support). Diagnoses carry on without hops.
    #[error("hop probing is not available: {0}")]
    Unavailable(String),
    /// This probe failed; later ones may work.
    #[error("the probe failed: {0}")]
    Failed(String),
}

/// Sends one ICMP echo with a limited time-to-live, like each probe of MTR.
pub trait HopProber: Send + Sync {
    fn probe(
        &self,
        target: IpAddr,
        ttl: u8,
        timeout: Duration,
    ) -> impl Future<Output = Result<ProbeOutcome, HopProbeError>> + Send;
}

/// The address a server name resolved to, and how long resolving took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    /// The host the game connects to (the SRV target when there is one).
    pub host: String,
    pub address: SocketAddr,
    /// How long looking up the host's address took.
    pub dns: Duration,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ConnectionError {
    #[error("the server's name could not be resolved: {0}")]
    NameNotFound(String),
    #[error("the connection failed: {0}")]
    Failed(String),
}

/// Times name resolution (SRV record included) and TCP connects.
pub trait ConnectionTimer: Send + Sync {
    fn resolve(
        &self,
        server: &ServerAddress,
    ) -> impl Future<Output = Result<ResolvedTarget, ConnectionError>> + Send;

    /// How long a TCP handshake with `address` takes; `Ok(None)` on timeout.
    fn connect(
        &self,
        address: SocketAddr,
        timeout: Duration,
    ) -> impl Future<Output = Result<Option<Duration>, ConnectionError>> + Send;
}

/// Receives the diagnosis as it progresses (about once per second).
pub trait DiagnosticSink: Send + Sync {
    fn update(&self, diagnostic: &Diagnostic);
}

/// A report published as a shareable link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedReport {
    pub url: String,
    /// When the link stops working (Unix seconds), when the host says.
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ReportShareError {
    #[error("the report could not be uploaded: {0}")]
    Unavailable(String),
    #[error("the report host refused the report: {0}")]
    Refused(String),
}

/// Publishes a plain-text report (pastes.dev).
pub trait ReportHost: Send + Sync {
    fn publish(
        &self,
        content: &str,
    ) -> impl Future<Output = Result<SharedReport, ReportShareError>> + Send;
}
