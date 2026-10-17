use std::fmt;

use thiserror::Error;

pub const DEFAULT_PORT: u16 = 25565;
const MAX_LEN: usize = 255;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServerAddressError {
    #[error("server address must not be empty")]
    Empty,
    #[error("server address is too long")]
    TooLong,
    #[error("invalid host in server address {0:?}")]
    InvalidHost(String),
    #[error("invalid port in server address {0:?}")]
    InvalidPort(String),
}

/// A server address as players type it: `host`, `host:port` or `[ipv6]:port`.
/// The port is kept only when the player wrote one, like vanilla.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerAddress {
    host: String,
    port: Option<u16>,
}

impl ServerAddress {
    pub fn parse(raw: &str) -> Result<Self, ServerAddressError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(ServerAddressError::Empty);
        }
        if raw.len() > MAX_LEN {
            return Err(ServerAddressError::TooLong);
        }

        let (host, port) = if let Some(rest) = raw.strip_prefix('[') {
            let (host, after) = rest
                .split_once(']')
                .ok_or_else(|| ServerAddressError::InvalidHost(raw.to_owned()))?;
            // Brackets are only for IPv6 literals, which always contain a colon.
            if !host.contains(':')
                || !host
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
            {
                return Err(ServerAddressError::InvalidHost(raw.to_owned()));
            }
            match after {
                "" => (host, None),
                _ => (
                    host,
                    Some(
                        after
                            .strip_prefix(':')
                            .ok_or_else(|| ServerAddressError::InvalidPort(raw.to_owned()))?,
                    ),
                ),
            }
        } else {
            match raw.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (raw, None),
            }
        };

        let is_ipv6 = host.contains(':');
        let valid_host = is_ipv6
            || (!host.is_empty()
                && !host.starts_with(['.', '-'])
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')));
        if !valid_host {
            return Err(ServerAddressError::InvalidHost(raw.to_owned()));
        }

        let port = port
            .map(|port| port.parse::<u16>().ok().filter(|port| *port != 0))
            .map(|port| port.ok_or_else(|| ServerAddressError::InvalidPort(raw.to_owned())))
            .transpose()?;
        Ok(Self {
            host: host.to_owned(),
            port,
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port to connect to: the written one or 25565.
    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_PORT)
    }

    /// Whether both addresses reach the same server (host case and default port ignored).
    pub fn same_server(&self, other: &Self) -> bool {
        self.host.eq_ignore_ascii_case(&other.host)
            && self.effective_port() == other.effective_port()
    }
}

impl fmt::Display for ServerAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        match self.port {
            Some(port) => write!(f, "{host}:{port}"),
            None => f.write_str(&host),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> ServerAddress {
        ServerAddress::parse(raw).unwrap()
    }

    #[test]
    fn parses_hosts_ports_and_ipv6_and_round_trips() {
        for raw in [
            "mc.policarpiers.com",
            "play.example.net:25570",
            "192.168.1.10:25565",
            "[::1]:25566",
            "[2001:db8::1]",
        ] {
            assert_eq!(parse(raw).to_string(), raw);
        }
        assert_eq!(
            parse("  mc.policarpiers.com ").effective_port(),
            DEFAULT_PORT
        );
        assert_eq!(parse("[::1]:25566").host(), "::1");
    }

    #[test]
    fn rejects_malformed_addresses() {
        assert_eq!(ServerAddress::parse(" "), Err(ServerAddressError::Empty));
        for raw in [
            "host:0",
            "host:65536",
            "host:abc",
            "[::1]x",
            "[::1",
            "[ab.cd]",
            "[1234]:25565",
        ] {
            assert!(ServerAddress::parse(raw).is_err(), "{raw}");
        }
        for raw in ["bad host", "-leading.dash", "../x", "a/b"] {
            assert!(
                matches!(
                    ServerAddress::parse(raw),
                    Err(ServerAddressError::InvalidHost(_))
                ),
                "{raw}"
            );
        }
        assert_eq!(
            ServerAddress::parse(&"a".repeat(256)),
            Err(ServerAddressError::TooLong)
        );
    }

    #[test]
    fn same_server_ignores_host_case_and_the_default_port() {
        assert!(parse("MC.Policarpiers.com").same_server(&parse("mc.policarpiers.com:25565")));
        assert!(!parse("mc.policarpiers.com").same_server(&parse("mc.policarpiers.com:25570")));
    }
}
