use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::{Instant, timeout};

use crate::domain::diagnostics::{ConnectionError, ConnectionTimer, ResolvedTarget};
use crate::domain::server_list::ServerAddress;
use crate::infrastructure::server_ping::{SrvResolver, SystemSrvResolver, connect_target};

/// The limit for the SRV lookup and, separately, for resolving the host.
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// Resolves servers like the game does (SRV record first) and times TCP
/// handshakes with their port.
pub struct SystemConnectionTimer<R = SystemSrvResolver> {
    resolver: R,
}

impl SystemConnectionTimer {
    pub fn new() -> Self {
        Self::with_resolver(SystemSrvResolver::new())
    }
}

impl Default for SystemConnectionTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: SrvResolver> SystemConnectionTimer<R> {
    pub fn with_resolver(resolver: R) -> Self {
        Self { resolver }
    }
}

impl<R: SrvResolver> ConnectionTimer for SystemConnectionTimer<R> {
    async fn resolve(&self, server: &ServerAddress) -> Result<ResolvedTarget, ConnectionError> {
        let (host, port) = connect_target(&self.resolver, server, RESOLVE_TIMEOUT).await;
        // Only the host lookup is timed: the SRV resolver's first query also
        // pays for building it (seconds on some systems), which says nothing
        // about the player's DNS.
        let started = Instant::now();
        let addresses: Vec<SocketAddr> = timeout(
            RESOLVE_TIMEOUT,
            tokio::net::lookup_host((host.as_str(), port)),
        )
        .await
        .map_err(|_| ConnectionError::NameNotFound(format!("{host}: timed out")))?
        .map_err(|error| ConnectionError::NameNotFound(format!("{host}: {error}")))?
        .collect();
        let dns = started.elapsed();
        let address = preferred_address(&addresses)
            .ok_or_else(|| ConnectionError::NameNotFound(format!("{host}: no address")))?;
        Ok(ResolvedTarget { host, address, dns })
    }

    async fn connect(
        &self,
        address: SocketAddr,
        limit: Duration,
    ) -> Result<Option<Duration>, ConnectionError> {
        let started = Instant::now();
        match timeout(limit, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => {
                let rtt = started.elapsed();
                drop(stream);
                Ok(Some(rtt))
            }
            Ok(Err(error)) => Err(ConnectionError::Failed(format!("{address}: {error}"))),
            Err(_) => Ok(None),
        }
    }
}

/// The address to diagnose: IPv4 first, because hop probing supports IPv4
/// only and most players reach servers over it.
fn preferred_address(addresses: &[SocketAddr]) -> Option<SocketAddr> {
    addresses
        .iter()
        .find(|address| address.is_ipv4())
        .or_else(|| addresses.first())
        .copied()
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;
    use crate::infrastructure::server_ping::SrvRecord;

    struct NoSrv;

    impl SrvResolver for NoSrv {
        async fn lookup_srv(&self, _name: &str) -> Result<Vec<SrvRecord>, String> {
            Err("no record".into())
        }
    }

    fn timer() -> SystemConnectionTimer<NoSrv> {
        SystemConnectionTimer::with_resolver(NoSrv)
    }

    #[tokio::test]
    async fn resolves_an_address_with_its_port() {
        let server = ServerAddress::parse("127.0.0.1:25570").unwrap();
        let target = timer().resolve(&server).await.unwrap();
        assert_eq!(target.address, "127.0.0.1:25570".parse().unwrap());
        assert_eq!(target.host, "127.0.0.1");
    }

    #[tokio::test]
    async fn an_unknown_name_is_not_found() {
        let server = ServerAddress::parse("no-such-host.invalid").unwrap();
        assert!(matches!(
            timer().resolve(&server).await,
            Err(ConnectionError::NameNotFound(_))
        ));
    }

    #[tokio::test]
    async fn times_a_connect_and_reports_a_refused_one() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let open = listener.local_addr().unwrap();
        let rtt = timer().connect(open, Duration::from_secs(1)).await.unwrap();
        assert!(rtt.is_some());
        drop(listener);
        // Windows retries the SYN after a reset and reports the refusal only
        // after about two seconds, so this limit must be longer than that.
        assert!(matches!(
            timer().connect(open, Duration::from_secs(5)).await,
            Err(ConnectionError::Failed(_))
        ));
    }

    #[test]
    fn prefers_ipv4() {
        let v6: SocketAddr = "[2001:db8::1]:25565".parse().unwrap();
        let v4: SocketAddr = "192.0.2.1:25565".parse().unwrap();
        assert_eq!(preferred_address(&[v6, v4]), Some(v4));
        assert_eq!(preferred_address(&[v6]), Some(v6));
        assert_eq!(preferred_address(&[]), None);
    }
}
