//! Network diagnostics adapters: hop probes (ICMP), DNS and TCP timing, and
//! the mclo.gs report host.

mod connection;
mod icmp;
mod mclogs;

pub use connection::{RESOLVE_TIMEOUT, SystemConnectionTimer};
pub use icmp::IcmpHopProber;
pub use mclogs::{MCLOGS_API_URL, MclogsReportHost};
