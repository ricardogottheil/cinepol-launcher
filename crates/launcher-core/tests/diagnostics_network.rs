//! Diagnoses the real path to the CINEPOL server for one minute and prints
//! the hops, so the ICMP prober can be checked on each operating system.
//!
//! Opt-in only: `cargo test -p cinepol-launcher-core --features network-tests --test diagnostics_network -- --nocapture`
//! (`CINEPOL_E2E_DIAGNOSE` for another server).
#![cfg(feature = "network-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::time::Duration;

use launcher_core::application::diagnose_connection::{DiagnoseDeps, diagnose_connection};
use launcher_core::domain::diagnostics::{Diagnostic, DiagnosticSink, DiagnosticState, HopProbing};
use launcher_core::infrastructure::diagnostics::{IcmpHopProber, SystemConnectionTimer};
use launcher_core::infrastructure::server_ping::MinecraftServerPinger;
use launcher_core::infrastructure::time::{SystemClock, TokioDelay};
use tokio_util::sync::CancellationToken;

struct Quiet;

impl DiagnosticSink for Quiet {
    fn update(&self, _diagnostic: &Diagnostic) {}
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnoses_the_cinepol_server() {
    let server =
        std::env::var("CINEPOL_E2E_DIAGNOSE").unwrap_or_else(|_| "mc.policarpiers.com".into());
    let hops = IcmpHopProber::new();
    let connections = SystemConnectionTimer::new();
    let status = MinecraftServerPinger::new();
    let deps = DiagnoseDeps {
        hops: &hops,
        connections: &connections,
        status: &status,
        delay: &TokioDelay,
        clock: &SystemClock,
    };
    let diagnostic = diagnose_connection(
        &server,
        Duration::from_secs(60),
        &deps,
        &Quiet,
        &CancellationToken::new(),
    )
    .await
    .unwrap();

    let target = diagnostic.target.as_ref().expect("the name resolves");
    println!("{server} -> {} (dns {:?})", target.address, target.dns);
    println!("hop probing: {:?}", diagnostic.hop_probing);
    for hop in &diagnostic.hops {
        println!(
            "{:>2} {:<18} {:?} loss {:>5.1}% avg {:?}",
            hop.ttl,
            hop.address
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "???".into()),
            hop.zone,
            hop.stats.loss_percent(),
            hop.stats.average_ms(),
        );
    }
    println!(
        "tcp loss {:.1}% avg {:?}; minecraft avg {:?}",
        diagnostic.tcp.loss_percent(),
        diagnostic.tcp.average_ms(),
        diagnostic.minecraft.average_ms()
    );
    println!("verdict: {:?}", diagnostic.verdict);

    assert_eq!(diagnostic.state, DiagnosticState::Finished);
    assert!(diagnostic.tcp.answered(), "the server's port answers");
    if diagnostic.hop_probing == HopProbing::Available {
        assert!(
            diagnostic.hops.iter().any(|hop| hop.stats.answered()),
            "at least one hop answers"
        );
    }
}
