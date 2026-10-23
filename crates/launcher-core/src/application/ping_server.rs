//! The live status of any server address (players, version, latency).

use thiserror::Error;

use crate::domain::server::{ServerStatus, ServerStatusError, ServerStatusProbe};
use crate::domain::server_list::{ServerAddress, ServerAddressError};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PingServerError {
    #[error(transparent)]
    InvalidAddress(#[from] ServerAddressError),
    #[error(transparent)]
    Status(#[from] ServerStatusError),
}

/// Checks the address before anything goes on the network.
pub async fn ping_server<P: ServerStatusProbe>(
    address: &str,
    probe: &P,
) -> Result<ServerStatus, PingServerError> {
    let address = ServerAddress::parse(address)?;
    Ok(probe.status(&address).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeServerStatusProbe;

    fn status() -> ServerStatus {
        ServerStatus {
            online_players: 9,
            max_players: 100,
            version_name: "Velocity 1.21.11-26.3".into(),
            protocol: 777,
            motd: "CINEPOL".into(),
            latency_ms: Some(57),
        }
    }

    #[tokio::test]
    async fn pings_a_valid_address() {
        let probe = FakeServerStatusProbe::answering(Ok(status()));
        assert_eq!(
            ping_server(" play.example.net:25566 ", &probe).await,
            Ok(status())
        );
        assert_eq!(probe.pinged(), ["play.example.net:25566"]);
    }

    #[tokio::test]
    async fn refuses_a_bad_address_without_pinging() {
        let probe = FakeServerStatusProbe::answering(Ok(status()));
        assert!(matches!(
            ping_server("bad host!", &probe).await,
            Err(PingServerError::InvalidAddress(_))
        ));
        assert!(probe.pinged().is_empty());
    }
}
