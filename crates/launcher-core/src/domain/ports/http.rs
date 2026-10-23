use std::future::Future;

use thiserror::Error;

use super::Secret;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum HttpError {
    #[error("{0} was not found")]
    NotFound(String),
    #[error("{url} answered HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("rate limited by {url}; retry after {retry_after_secs} s")]
    RateLimited { url: String, retry_after_secs: u64 },
    #[error("request to {url} failed: {reason}")]
    Network { url: String, reason: String },
    #[error("{url} answered more than {limit} bytes")]
    TooLarge { url: String, limit: usize },
}

/// Small metadata requests (JSON manifests). Large files go through `Downloader`.
/// Implementations send the launcher's descriptive User-Agent and retry
/// transient failures.
pub trait HttpClient: Send + Sync {
    fn get_bytes(&self, url: &str) -> impl Future<Output = Result<Vec<u8>, HttpError>> + Send;

    /// Like [`HttpClient::get_bytes`], but refuses a body larger than `limit`
    /// with [`HttpError::TooLarge`] (never retried). Network clients stop
    /// reading at the cap; the default checks the buffered body, for fakes.
    fn get_bytes_limited(
        &self,
        url: &str,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<u8>, HttpError>> + Send {
        async move {
            let bytes = self.get_bytes(url).await?;
            if bytes.len() > limit {
                return Err(HttpError::TooLarge {
                    url: url.to_owned(),
                    limit,
                });
            }
            Ok(bytes)
        }
    }

    /// POSTs a JSON document (`application/json`) and returns the answer body,
    /// with the same User-Agent, retries and rate-limit handling as
    /// [`HttpClient::get_bytes`]. Because it may be retried, it is only for
    /// idempotent queries (lookups such as Modrinth `version_files`), never
    /// for exchanges that must happen once (see [`HttpRequester`]).
    ///
    /// The default refuses the request, for clients that only read.
    fn post_json(
        &self,
        url: &str,
        body: Vec<u8>,
    ) -> impl Future<Output = Result<Vec<u8>, HttpError>> + Send {
        drop(body);
        let url = url.to_owned();
        async move {
            Err(HttpError::Network {
                url,
                reason: "this HTTP client does not send POST requests".into(),
            })
        }
    }

    /// [`HttpClient::get_bytes`] with an API key header, for catalogs that
    /// need one (CurseForge `x-api-key`). The key never appears in errors.
    ///
    /// The default refuses the request, so no client sends a key by accident.
    fn get_bytes_keyed(
        &self,
        url: &str,
        key: &ApiKeyHeader,
    ) -> impl Future<Output = Result<Vec<u8>, HttpError>> + Send {
        let _ = key;
        let url = url.to_owned();
        async move { Err(no_keyed_requests(url)) }
    }

    /// [`HttpClient::post_json`] with an API key header.
    fn post_json_keyed(
        &self,
        url: &str,
        body: Vec<u8>,
        key: &ApiKeyHeader,
    ) -> impl Future<Output = Result<Vec<u8>, HttpError>> + Send {
        let _ = (body, key);
        let url = url.to_owned();
        async move { Err(no_keyed_requests(url)) }
    }
}

fn no_keyed_requests(url: String) -> HttpError {
    HttpError::Network {
        url,
        reason: "this HTTP client does not send API keys".into(),
    }
}

/// One header carrying an API key (`name: value`). The value is a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyHeader {
    pub name: &'static str,
    pub value: Secret,
}

impl ApiKeyHeader {
    pub fn new(name: &'static str, value: Secret) -> Self {
        Self { name, value }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

/// One field of a `multipart/form-data` body: plain text, or a file with its
/// name and media type. Names and media types must not hold quotes or line
/// breaks; the adapter refuses them rather than send a broken form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

impl MultipartPart {
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            filename: None,
            content_type: None,
            data: value.into().into_bytes(),
        }
    }

    pub fn file(
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: Vec<u8>,
    ) -> Self {
        Self {
            name: name.into(),
            filename: Some(filename.into()),
            content_type: Some(content_type.into()),
            data,
        }
    }
}

/// A request body. Form and JSON bodies carry tokens, so they are kept as
/// secrets; multipart bodies carry uploads (a skin), never credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpBody {
    /// `application/x-www-form-urlencoded` fields.
    Form(Vec<(String, Secret)>),
    /// An `application/json` document.
    Json(Secret),
    /// A `multipart/form-data` form.
    Multipart(Vec<MultipartPart>),
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

    pub fn put(url: impl Into<String>, body: HttpBody) -> Self {
        Self {
            method: HttpMethod::Put,
            body: Some(body),
            ..Self::get(url)
        }
    }

    pub fn delete(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Delete,
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
