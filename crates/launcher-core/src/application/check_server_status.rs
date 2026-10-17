//! Pings the CINEPOL server for its live status.

use crate::domain::server::{ServerConfig, ServerStatus, ServerStatusError, ServerStatusProbe};

pub async fn check_server_status<P: ServerStatusProbe>(
    probe: &P,
    config: &ServerConfig,
) -> Result<ServerStatus, ServerStatusError> {
    probe.status(&config.address).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::load_server_config::fixtures::cinepol;
    use crate::testing::FakeServerStatusProbe;

    #[tokio::test]
    async fn pings_the_configured_address() {
        let status = ServerStatus {
            online_players: 11,
            max_players: 100,
            version_name: "Velocity".into(),
            protocol: 777,
            motd: "CINEPOL".into(),
            latency_ms: Some(40),
        };
        let probe = FakeServerStatusProbe::answering(Ok(status.clone()));
        assert_eq!(
            check_server_status(&probe, &cinepol("26.2")).await,
            Ok(status)
        );
        assert_eq!(probe.pinged(), ["mc.policarpiers.com"]);
    }
}
