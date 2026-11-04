//! Hop probes without administrator rights: ICMP echo requests with a
//! limited time-to-live, sent through an unprivileged ICMP socket on macOS
//! and Linux and through `IcmpSendEcho` on Windows (the API `tracert` uses).

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use crate::domain::diagnostics::{HopProbeError, HopProber, ProbeOutcome};

/// Marks the launcher's echo requests so replies to other programs are ignored.
const PAYLOAD_TAG: &[u8; 12] = b"CINEPOL-DIAG";
const ECHO_REQUEST: u8 = 8;
const ECHO_REPLY: u8 = 0;
const TIME_EXCEEDED: u8 = 11;
const DESTINATION_UNREACHABLE: u8 = 3;

/// Every probe gets its own sequence number, so concurrent probes never take
/// each other's replies.
static SEQUENCE: AtomicU16 = AtomicU16::new(1);

fn next_sequence() -> u16 {
    SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

/// Probes hops of IPv4 paths.
#[derive(Debug, Clone, Copy)]
pub struct IcmpHopProber {
    ident: u16,
}

impl IcmpHopProber {
    pub fn new() -> Self {
        Self {
            ident: (std::process::id() & 0xffff) as u16,
        }
    }
}

impl Default for IcmpHopProber {
    fn default() -> Self {
        Self::new()
    }
}

impl HopProber for IcmpHopProber {
    async fn probe(
        &self,
        target: IpAddr,
        ttl: u8,
        timeout: Duration,
    ) -> Result<ProbeOutcome, HopProbeError> {
        let IpAddr::V4(target) = target else {
            return Err(HopProbeError::Unavailable(
                "hop probing supports IPv4 servers only".into(),
            ));
        };
        let ident = self.ident;
        let sequence = next_sequence();
        // The probe blocks for up to `timeout`; keep it off the async workers.
        tokio::task::spawn_blocking(move || probe_blocking(target, ttl, timeout, ident, sequence))
            .await
            .map_err(|error| HopProbeError::Failed(error.to_string()))?
    }
}

#[cfg(unix)]
fn probe_blocking(
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    ident: u16,
    sequence: u16,
) -> Result<ProbeOutcome, HopProbeError> {
    unix::probe(target, ttl, timeout, ident, sequence)
}

#[cfg(windows)]
fn probe_blocking(
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    _ident: u16,
    _sequence: u16,
) -> Result<ProbeOutcome, HopProbeError> {
    windows::probe(target, ttl, timeout)
}

#[cfg(not(any(unix, windows)))]
fn probe_blocking(
    _target: Ipv4Addr,
    _ttl: u8,
    _timeout: Duration,
    _ident: u16,
    _sequence: u16,
) -> Result<ProbeOutcome, HopProbeError> {
    Err(HopProbeError::Unavailable(
        "hop probing is not supported on this system".into(),
    ))
}

/// The payload every echo request carries: the tag and the sequence number.
fn payload(sequence: u16) -> Vec<u8> {
    let mut payload = PAYLOAD_TAG.to_vec();
    payload.extend_from_slice(&sequence.to_be_bytes());
    payload.resize(32, 0);
    payload
}

/// An ICMP echo request with its checksum.
#[cfg_attr(windows, allow(dead_code))]
fn echo_request(ident: u16, sequence: u16) -> Vec<u8> {
    let mut packet = vec![ECHO_REQUEST, 0, 0, 0];
    packet.extend_from_slice(&ident.to_be_bytes());
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&payload(sequence));
    let checksum = internet_checksum(&packet);
    packet[2..4].copy_from_slice(&checksum.to_be_bytes());
    packet
}

/// RFC 1071 checksum.
fn internet_checksum(bytes: &[u8]) -> u16 {
    let mut sum: u32 = bytes
        .chunks(2)
        .map(|pair| u32::from(u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)])))
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// What a received datagram says about the probe `sequence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(windows, allow(dead_code))]
enum Reply {
    /// The target answered the echo.
    Echo,
    /// A router on the way dropped it (time-to-live exceeded) or refused it.
    Router,
}

/// Reads an ICMP message, with or without its IPv4 header (macOS includes
/// it, Linux does not), and tells whether it answers probe `sequence`.
/// Kernels may rewrite the identifier of unprivileged echo requests, so
/// replies match on the sequence number (echo replies also on the tag).
#[cfg_attr(windows, allow(dead_code))]
fn parse_reply(datagram: &[u8], sequence: u16) -> Option<Reply> {
    let icmp = strip_ipv4_header(datagram)?;
    let kind = *icmp.first()?;
    match kind {
        ECHO_REPLY => {
            let seq = u16::from_be_bytes([*icmp.get(6)?, *icmp.get(7)?]);
            let tagged = icmp.get(8..8 + PAYLOAD_TAG.len()) == Some(PAYLOAD_TAG.as_slice());
            (seq == sequence && tagged).then_some(Reply::Echo)
        }
        TIME_EXCEEDED | DESTINATION_UNREACHABLE => {
            // 8 bytes of ICMP header, then the dropped packet's IPv4 header
            // and the first 8 bytes of the echo request.
            let inner = strip_ipv4_header(icmp.get(8..)?)?;
            let inner_kind = *inner.first()?;
            let seq = u16::from_be_bytes([*inner.get(6)?, *inner.get(7)?]);
            (inner_kind == ECHO_REQUEST && seq == sequence).then_some(Reply::Router)
        }
        _ => None,
    }
}

/// The datagram after its IPv4 header, when it starts with one.
#[cfg_attr(windows, allow(dead_code))]
fn strip_ipv4_header(datagram: &[u8]) -> Option<&[u8]> {
    let first = *datagram.first()?;
    if first >> 4 == 4 {
        let length = usize::from(first & 0x0f) * 4;
        if length < 20 {
            return None;
        }
        datagram.get(length..)
    } else {
        Some(datagram)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ipv4_header() -> Vec<u8> {
        let mut header = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0];
        header.extend_from_slice(&[192, 168, 1, 1, 10, 0, 0, 2]);
        header
    }

    fn echo_reply(sequence: u16) -> Vec<u8> {
        let mut reply = echo_request(0x4242, sequence);
        reply[0] = ECHO_REPLY;
        reply
    }

    fn time_exceeded(sequence: u16) -> Vec<u8> {
        let mut message = vec![TIME_EXCEEDED, 0, 0, 0, 0, 0, 0, 0];
        message.extend(ipv4_header());
        message.extend_from_slice(&echo_request(0x4242, sequence)[..8]);
        message
    }

    #[test]
    fn the_checksum_of_a_request_verifies() {
        let packet = echo_request(0x1234, 7);
        assert_eq!(internet_checksum(&packet), 0);
        assert_eq!(packet[0], ECHO_REQUEST);
        assert_eq!(&packet[8..20], PAYLOAD_TAG);
    }

    #[test]
    fn matches_echo_replies_with_and_without_an_ip_header() {
        let bare = echo_reply(9);
        let mut framed = ipv4_header();
        framed.extend(&bare);
        assert_eq!(parse_reply(&bare, 9), Some(Reply::Echo));
        assert_eq!(parse_reply(&framed, 9), Some(Reply::Echo));
        assert_eq!(parse_reply(&bare, 10), None);
    }

    #[test]
    fn matches_time_exceeded_messages_on_the_inner_sequence() {
        let bare = time_exceeded(300);
        let mut framed = ipv4_header();
        framed.extend(&bare);
        assert_eq!(parse_reply(&bare, 300), Some(Reply::Router));
        assert_eq!(parse_reply(&framed, 300), Some(Reply::Router));
        assert_eq!(parse_reply(&framed, 301), None);
    }

    #[test]
    fn ignores_untagged_replies_and_garbage() {
        let mut foreign = echo_reply(9);
        foreign[8] = b'X';
        assert_eq!(parse_reply(&foreign, 9), None);
        assert_eq!(parse_reply(&[], 9), None);
        assert_eq!(parse_reply(&[0x45, 0, 0], 9), None);
        assert_eq!(parse_reply(&[0x41; 30], 9), None);
    }

    /// Probes the loopback for real: proves the socket path works on this
    /// machine (skipped where the system forbids unprivileged ICMP).
    #[tokio::test]
    async fn probes_the_loopback() {
        let prober = IcmpHopProber::new();
        match prober
            .probe(IpAddr::V4(Ipv4Addr::LOCALHOST), 64, Duration::from_secs(1))
            .await
        {
            Ok(ProbeOutcome::Reply { from, reached, .. }) => {
                assert_eq!(from, IpAddr::V4(Ipv4Addr::LOCALHOST));
                assert!(reached);
            }
            Err(HopProbeError::Unavailable(reason)) => {
                eprintln!("skipped: {reason}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
