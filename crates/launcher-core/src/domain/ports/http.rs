use std::future::Future;

use thiserror::Error;

use super::Secret;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

/// A request body. Both kinds carry tokens, so they are kept as secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpBody {
    /// `application/x-www-form-urlencoded` fields.
    Form(Vec<(String, Secret)>),
    /// An `application/json` document.
    Json(Secret),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub body: Option<HttpBody>,
    /// Sent as `Authorization: Bearer <token>`.
    pub bearer: Option<Secret>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            url: url.into(),
            body: None,
            bearer: None,
        }
    }

    pub fn post(url: impl Into<String>, body: HttpBody) -> Self {
        Self {
            method: HttpMethod::Post,
            body: Some(body),
            ..Self::get(url)
        }
    }

    pub fn with_bearer(mut self, token: &Secret) -> Self {
        self.bearer = Some(token.clone());
        self
    }
}

/// Any status the server answered, with its body (error details live there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// One request, sent exactly once. Sign-in exchanges must not be repeated
/// behind the caller's back (a code or refresh token is single use), so this
/// never retries and returns every status with its body. Only transport
/// failures are errors (`HttpError::Network`).
pub trait HttpRequester: Send + Sync {
    fn send(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, HttpError>> + Send;
}
