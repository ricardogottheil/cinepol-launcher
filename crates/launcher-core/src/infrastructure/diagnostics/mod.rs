//! Network diagnostics adapters: hop probes (ICMP), DNS and TCP timing, and
//! the pastes.dev report host.

mod connection;
mod icmp;
mod pastes;

pub use connection::{RESOLVE_TIMEOUT, SystemConnectionTimer};
pub use icmp::IcmpHopProber;
pub use pastes::{PASTES_API_URL, PASTES_VIEW_URL, PastesReportHost};
