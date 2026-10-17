use std::future::Future;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HttpError {
    #[error("{0} was not found")]
    NotFound(String),
    #[error("{url} answered HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("rate limited by {url}; retry after {retry_after_secs} s")]
    RateLimited { url: String, retry_after_secs: u64 },
    #[error("request to {url} failed: {reason}")]
    Network { url: String, reason: String },
}

/// Small metadata requests (JSON manifests). Large files go through `Downloader`.
/// Implementations send the launcher's descriptive User-Agent and retry
/// transient failures.
pub trait HttpClient: Send + Sync {
    fn get_bytes(&self, url: &str) -> impl Future<Output = Result<Vec<u8>, HttpError>> + Send;
}
