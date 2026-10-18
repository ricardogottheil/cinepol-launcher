//! The Server List Ping exchange: handshake, status request, ping/pong.
//! See https://minecraft.wiki/w/Java_Edition_protocol/Server_List_Ping.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout, timeout_at};

use super::codec::{FrameError, decode_string, encode_string, encode_varint, frame, read_packet};
use super::srv::{SrvResolver, SystemSrvResolver, best_target};
use super::status_json::parse_status;
use crate::domain::server::{ServerStatus, ServerStatusError, ServerStatusProbe};
use crate::domain::server_list::ServerAddress;

/// The default limit for one SRV lookup, one connect attempt and, separately,
/// the whole exchange. Name resolution plus every connect attempt share twice
/// this limit, so with the defaults a ping gives up after at most
/// 5 s (SRV) + 10 s (resolve and connect) + 5 s (exchange).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

const HANDSHAKE_ID: i32 = 0x00;
const STATUS_ID: i32 = 0x00;
const PING_ID: i32 = 0x01;
/// "Any version": servers answer the status of whatever they run.
const ANY_PROTOCOL: i32 = -1;
const NEXT_STATE_STATUS: i32 = 1;
const PING_PAYLOAD: i64 = 0x0043_494e_4550_4f4c;

/// Pings Minecraft servers over TCP, following `_minecraft._tcp` SRV records.
pub struct MinecraftServerPinger<R = SystemSrvResolver> {
    resolver: R,
    connect_timeout: Duration,
    exchange_timeout: Duration,
}

impl MinecraftServerPinger {
    /// Uses the operating system's DNS configuration for SRV lookups.
    pub fn new() -> Self {
        Self::with_resolver(SystemSrvResolver::new())
    }
}

impl Default for MinecraftServerPinger {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: SrvResolver> MinecraftServerPinger<R> {
    pub fn with_resolver(resolver: R) -> Self {
        Self {
            resolver,
            connect_timeout: DEFAULT_TIMEOUT,
            exchange_timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Sets the connect timeout (the SRV lookup and each connect attempt; name
    /// resolution plus all attempts share twice it) and the timeout of the
    /// whole status exchange.
    pub fn with_timeouts(mut self, connect: Duration, exchange: Duration) -> Self {
        self.connect_timeout = connect;
        self.exchange_timeout = exchange;
        self
    }

    /// Where to connect: the written port directly, otherwise the SRV record,
    /// otherwise the host on 25565.
    async fn connect_target(&self, address: &ServerAddress) -> (String, u16) {
        let host = address.host();
        let direct = (host.to_owned(), address.effective_port());
        if has_explicit_port(address) || host.parse::<IpAddr>().is_ok() {
            return direct;
        }
        let name = format!("_minecraft._tcp.{host}");
        match timeout(self.connect_timeout, self.resolver.lookup_srv(&name)).await {
            Ok(Ok(records)) => best_target(&records).unwrap_or(direct),
            Ok(Err(error)) => {
                tracing::debug!(%name, %error, "SRV lookup failed, using the default port");
                direct
            }
            Err(_) => {
                tracing::debug!(%name, "SRV lookup timed out, using the default port");
                direct
            }
        }
    }
}

impl<R: SrvResolver> ServerStatusProbe for MinecraftServerPinger<R> {
    async fn status(&self, address: &ServerAddress) -> Result<ServerStatus, ServerStatusError> {
        let (host, port) = self.connect_target(address).await;
        let budget = self.connect_timeout * 2;
        let started = Instant::now();
        let addrs: Vec<SocketAddr> =
            timeout(budget, tokio::net::lookup_host((host.as_str(), port)))
                .await
                .map_err(|_| unreachable_at(&host, port, "timed out resolving the host"))?
                .map_err(|error| unreachable_at(&host, port, &error.to_string()))?
                .collect();
        let remaining = budget.saturating_sub(started.elapsed());
        let mut stream = connect_first(&addrs, self.connect_timeout, remaining, TcpStream::connect)
            .await
            .map_err(|reason| unreachable_at(&host, port, &reason))?;

        let deadline = Instant::now() + self.exchange_timeout;
        let mut status = timeout_at(deadline, request_status(&mut stream, address.host(), port))
            .await
            .map_err(|_| unreachable_at(&host, port, "timed out waiting for the status"))??;
        status.latency_ms = timeout_at(deadline, measure_latency(&mut stream))
            .await
            .ok()
            .flatten();
        Ok(status)
    }
}

/// Tries each address in order, each for at most `per_attempt` and all of them
/// within `budget`, so one unresponsive address (often an IPv6 one without a
/// route) does not hide the next.
async fn connect_first<T, F, Fut>(
    addrs: &[SocketAddr],
    per_attempt: Duration,
    budget: Duration,
    mut connect: F,
) -> Result<T, String>
where
    F: FnMut(SocketAddr) -> Fut,
    Fut: Future<Output = std::io::Result<T>>,
{
    let deadline = Instant::now() + budget;
    let mut last = "the host has no address".to_owned();
    for &addr in addrs {
        let now = Instant::now();
        if now >= deadline {
            last = "timed out connecting".to_owned();
            break;
        }
        match timeout_at((now + per_attempt).min(deadline), connect(addr)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => last = format!("{addr}: {error}"),
            Err(_) => last = format!("{addr}: timed out connecting"),
        }
    }
    Err(last)
}

/// Whether the player wrote a port (`ServerAddress` keeps it only then).
fn has_explicit_port(address: &ServerAddress) -> bool {
    let host = address.host();
    let bare = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    address.to_string() != bare
}

fn unreachable_at(host: &str, port: u16, reason: &str) -> ServerStatusError {
    ServerStatusError::Unreachable(format!("{host}:{port}: {reason}"))
}

fn frame_error(error: FrameError) -> ServerStatusError {
    match error {
        FrameError::Io(reason) => ServerStatusError::Unreachable(reason),
        other => ServerStatusError::Protocol(other.to_string()),
    }
}

async fn request_status<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    port: u16,
) -> Result<ServerStatus, ServerStatusError> {
    let mut handshake = Vec::new();
    encode_varint(ANY_PROTOCOL, &mut handshake);
    encode_string(host, &mut handshake);
    handshake.extend_from_slice(&port.to_be_bytes());
    encode_varint(NEXT_STATE_STATUS, &mut handshake);

    let mut request = frame(HANDSHAKE_ID, &handshake);
    request.extend_from_slice(&frame(STATUS_ID, &[]));
    stream
        .write_all(&request)
        .await
        .map_err(|error| ServerStatusError::Unreachable(error.to_string()))?;

    let (id, body) = read_packet(stream).await.map_err(frame_error)?;
    if id != STATUS_ID {
        return Err(ServerStatusError::Protocol(format!(
            "expected the status response, got packet {id:#04x}"
        )));
    }
    let (json, _) = decode_string(&body).map_err(frame_error)?;
    parse_status(&json).map_err(|error| ServerStatusError::Protocol(error.to_string()))
}

/// The round trip of one ping, or `None` when the server does not answer it.
async fn measure_latency<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Option<u32> {
    let payload = PING_PAYLOAD.to_be_bytes();
    let started = Instant::now();
    stream.write_all(&frame(PING_ID, &payload)).await.ok()?;
    let (id, body) = read_packet(stream).await.ok()?;
    if id != PING_ID || body != payload {
        return None;
    }
    Some(u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::super::codec::decode_varint;
    use super::super::srv::SrvRecord;
    use super::*;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/server_ping/cinepol-status.json");

    enum Reply {
        /// Answers the status, and the ping when `pong` is set.
        Status { json: String, pong: bool },
        /// Accepts the connection and never writes.
        Silent,
        /// Reads the request, then writes these bytes.
        Raw(Vec<u8>),
    }

    /// A one-connection fake Minecraft server on 127.0.0.1. Its task returns the
    /// handshake payload it received.
    async fn fake_server(reply: Reply) -> (u16, JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        (port, tokio::spawn(serve(listener, reply)))
    }

    async fn serve(listener: TcpListener, reply: Reply) -> Vec<u8> {
        let (mut stream, _) = listener.accept().await.unwrap();
        match reply {
            Reply::Silent => {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Vec::new()
            }
            Reply::Raw(bytes) => {
                let mut request = [0u8; 16];
                let _ = stream.read(&mut request).await;
                stream.write_all(&bytes).await.unwrap();
                tokio::time::sleep(Duration::from_secs(1)).await;
                Vec::new()
            }
            Reply::Status { json, pong } => {
                let (id, handshake) = read_packet(&mut stream).await.unwrap();
                assert_eq!(id, HANDSHAKE_ID);
                let (id, request) = read_packet(&mut stream).await.unwrap();
                assert_eq!((id, request.len()), (STATUS_ID, 0));
                let mut response = Vec::new();
                encode_string(&json, &mut response);
                stream
                    .write_all(&frame(STATUS_ID, &response))
                    .await
                    .unwrap();
                if pong {
                    let (id, payload) = read_packet(&mut stream).await.unwrap();
                    assert_eq!(id, PING_ID);
                    stream.write_all(&frame(PING_ID, &payload)).await.unwrap();
                }
                handshake
            }
        }
    }

    /// Decodes a handshake payload into (protocol, host, port, next state).
    fn decode_handshake(payload: &[u8]) -> (i32, String, u16, i32) {
        let (protocol, used) = decode_varint(payload).unwrap();
        let (host, host_len) = decode_string(&payload[used..]).unwrap();
        let rest = &payload[used + host_len..];
        let port = u16::from_be_bytes([rest[0], rest[1]]);
        let (next, _) = decode_varint(&rest[2..]).unwrap();
        (protocol, host, port, next)
    }

    /// Records the names it is asked for and answers with fixed records.
    struct FakeResolver {
        answer: Result<Vec<SrvRecord>, String>,
        asked: Mutex<Vec<String>>,
    }

    impl FakeResolver {
        fn new(answer: Result<Vec<SrvRecord>, String>) -> Self {
            Self {
                answer,
                asked: Mutex::new(Vec::new()),
            }
        }

        fn pointing_to(port: u16) -> Self {
            Self::new(Ok(vec![SrvRecord {
                priority: 0,
                weight: 5,
                port,
                target: "127.0.0.1.".to_owned(),
            }]))
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl SrvResolver for FakeResolver {
        async fn lookup_srv(&self, name: &str) -> Result<Vec<SrvRecord>, String> {
            self.asked.lock().unwrap().push(name.to_owned());
            self.answer.clone()
        }
    }

    fn pinger(resolver: FakeResolver) -> MinecraftServerPinger<FakeResolver> {
        MinecraftServerPinger::with_resolver(resolver)
    }

    fn no_srv() -> FakeResolver {
        FakeResolver::new(Err("no record".to_owned()))
    }

    fn address(raw: &str) -> ServerAddress {
        ServerAddress::parse(raw).unwrap()
    }

    fn status_reply(json: &str, pong: bool) -> Reply {
        Reply::Status {
            json: json.to_owned(),
            pong,
        }
    }

    #[tokio::test]
    async fn reads_the_real_cinepol_status_and_measures_latency() {
        let (port, server) = fake_server(status_reply(FIXTURE, true)).await;
        let status = pinger(no_srv())
            .status(&address(&format!("127.0.0.1:{port}")))
            .await
            .unwrap();
        assert_eq!(status.online_players, 11);
        assert_eq!(status.max_players, 100);
        assert_eq!(status.version_name, "Velocity-CTD 1.21.11-26.3");
        assert_eq!(status.protocol, 777);
        assert!(status.motd.contains("ᴄɪɴᴇᴘᴏʟ"), "{}", status.motd);
        assert!(!status.motd.contains('§'));
        assert!(status.latency_ms.is_some());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn the_handshake_carries_the_host_as_written_and_the_port() {
        let (port, server) = fake_server(status_reply(FIXTURE, true)).await;
        pinger(no_srv())
            .status(&address(&format!("127.0.0.1:{port}")))
            .await
            .unwrap();
        let handshake = decode_handshake(&server.await.unwrap());
        assert_eq!(handshake, (-1, "127.0.0.1".to_owned(), port, 1));
    }

    #[tokio::test]
    async fn a_server_that_skips_the_pong_still_reports_its_status() {
        let json = r#"{"version":{"name":"1.21.11","protocol":774},"description":"Hi"}"#;
        let (port, server) = fake_server(status_reply(json, false)).await;
        let status = pinger(no_srv())
            .status(&address(&format!("127.0.0.1:{port}")))
            .await
            .unwrap();
        assert_eq!(status.motd, "Hi");
        assert_eq!((status.online_players, status.max_players), (0, 0));
        assert_eq!(status.latency_ms, None);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_silent_server_is_unreachable_within_the_timeout() {
        let (port, _server) = fake_server(Reply::Silent).await;
        let timeout = Duration::from_millis(200);
        let started = std::time::Instant::now();
        let result = pinger(no_srv())
            .with_timeouts(timeout, timeout)
            .status(&address(&format!("127.0.0.1:{port}")))
            .await;
        assert!(
            matches!(result, Err(ServerStatusError::Unreachable(_))),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn garbage_bytes_are_a_protocol_error() {
        let garbage = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n".repeat(4);
        let (port, _server) = fake_server(Reply::Raw(garbage)).await;
        let result = pinger(no_srv())
            .status(&address(&format!("127.0.0.1:{port}")))
            .await;
        assert!(
            matches!(result, Err(ServerStatusError::Protocol(_))),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn invalid_status_json_is_a_protocol_error() {
        let (port, _server) = fake_server(status_reply("{not json", false)).await;
        let result = pinger(no_srv())
            .status(&address(&format!("127.0.0.1:{port}")))
            .await;
        assert!(
            matches!(result, Err(ServerStatusError::Protocol(_))),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn an_address_without_a_port_follows_its_srv_record() {
        let (port, server) = fake_server(status_reply(FIXTURE, true)).await;
        let resolver = FakeResolver::pointing_to(port);
        let pinger = pinger(resolver);
        let status = pinger.status(&address("play.example.test")).await.unwrap();
        assert_eq!(status.online_players, 11);
        assert_eq!(
            pinger.resolver.asked(),
            ["_minecraft._tcp.play.example.test"]
        );
        // The handshake names the server as written, not the SRV target.
        let handshake = decode_handshake(&server.await.unwrap());
        assert_eq!(handshake, (-1, "play.example.test".to_owned(), port, 1));
    }

    #[tokio::test]
    async fn an_explicit_port_skips_the_srv_lookup() {
        let (port, server) = fake_server(status_reply(FIXTURE, true)).await;
        // The record points elsewhere; following it would fail.
        let pinger = pinger(FakeResolver::pointing_to(1));
        let status = pinger
            .status(&address(&format!("localhost:{port}")))
            .await
            .unwrap();
        assert_eq!(status.protocol, 777);
        assert!(pinger.resolver.asked().is_empty());
        let (_, host, _, _) = decode_handshake(&server.await.unwrap());
        assert_eq!(host, "localhost");
    }

    #[tokio::test]
    async fn without_a_usable_srv_record_the_default_port_is_used() {
        for resolver in [no_srv(), FakeResolver::new(Ok(Vec::new()))] {
            let pinger = pinger(resolver);
            let target = pinger.connect_target(&address("play.example.test")).await;
            assert_eq!(target, ("play.example.test".to_owned(), 25565));
            assert_eq!(pinger.resolver.asked().len(), 1);
        }
    }

    #[tokio::test]
    async fn ip_literals_skip_the_srv_lookup() {
        let pinger = pinger(FakeResolver::pointing_to(1));
        for raw in ["127.0.0.1", "[::1]"] {
            let target = pinger.connect_target(&address(raw)).await;
            assert_eq!(target.1, 25565, "{raw}");
        }
        assert!(pinger.resolver.asked().is_empty());
    }

    #[tokio::test]
    async fn pings_an_ipv6_literal() {
        // Skipped where the machine has no IPv6 loopback.
        let Ok(listener) = TcpListener::bind("[::1]:0").await else {
            return;
        };
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(serve(listener, status_reply(FIXTURE, true)));
        let status = pinger(no_srv())
            .status(&address(&format!("[::1]:{port}")))
            .await
            .unwrap();
        assert_eq!(status.max_players, 100);
        let (_, host, _, _) = decode_handshake(&server.await.unwrap());
        assert_eq!(host, "::1");
    }

    #[tokio::test]
    async fn a_hanging_address_does_not_use_up_the_whole_connect_budget() {
        let addrs: Vec<SocketAddr> = vec![
            "192.0.2.1:25565".parse().unwrap(),
            "127.0.0.1:25565".parse().unwrap(),
        ];
        let attempts = Mutex::new(Vec::new());
        let connected = connect_first(
            &addrs,
            Duration::from_millis(50),
            Duration::from_millis(500),
            |addr| {
                attempts.lock().unwrap().push(addr);
                async move {
                    if addr.ip().is_loopback() {
                        Ok(addr)
                    } else {
                        std::future::pending().await
                    }
                }
            },
        )
        .await;
        assert_eq!(connected.unwrap(), addrs[1]);
        assert_eq!(*attempts.lock().unwrap(), addrs);
    }

    #[tokio::test]
    async fn the_connect_budget_bounds_every_attempt_together() {
        let addrs: Vec<SocketAddr> = (1..=10)
            .map(|n| format!("192.0.2.{n}:25565").parse().unwrap())
            .collect();
        let started = Instant::now();
        let result = connect_first(
            &addrs,
            Duration::from_millis(50),
            Duration::from_millis(120),
            |_| async { std::future::pending::<std::io::Result<SocketAddr>>().await },
        )
        .await;
        assert!(result.is_err());
        assert!(
            started.elapsed() < Duration::from_millis(400),
            "{:?}",
            started.elapsed()
        );
    }
}
