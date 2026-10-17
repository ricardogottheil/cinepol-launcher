use std::future::Future;

use thiserror::Error;

use super::{HttpsUrl, ServerConfig, ServerStatus};
use crate::domain::server_list::ServerAddress;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ServerConfigError {
    #[error("the server config could not be fetched: {0}")]
    Unavailable(String),
    #[error("the server config is invalid: {0}")]
    Invalid(String),
}

/// Fetches and validates `server-config.json` from a URL.
pub trait ServerConfigSource: Send + Sync {
    fn fetch(
        &self,
        url: &HttpsUrl,
    ) -> impl Future<Output = Result<ServerConfig, ServerConfigError>> + Send;
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ServerStatusError {
    #[error("the server did not answer: {0}")]
    Unreachable(String),
    #[error("the server answered something unexpected: {0}")]
    Protocol(String),
}

/// Pings a Minecraft server with the Server List Ping protocol, following its
/// `_minecraft._tcp` SRV record when the address has no explicit port.
pub trait ServerStatusProbe: Send + Sync {
    fn status(
        &self,
        address: &ServerAddress,
    ) -> impl Future<Output = Result<ServerStatus, ServerStatusError>> + Send;
}
