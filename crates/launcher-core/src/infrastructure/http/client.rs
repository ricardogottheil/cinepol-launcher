//! `HttpClient` over reqwest, with retries for transient failures.

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;

use crate::domain::ports::{HttpClient, HttpError};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ATTEMPTS: u32 = 3;
const DEFAULT_BACKOFF: Duration = Duration::from_millis(500);
const MAX_RETRY_AFTER_SECS: u64 = 60;

/// Sends the launcher's User-Agent and retries network errors, HTTP 5xx and
/// 429 up to three attempts with exponential backoff.
#[derive(Debug, Clone)]
pub struct ReqwestHttpClient {
    client: reqwest::Client,
    initial_backoff: Duration,
}

impl ReqwestHttpClient {
    pub fn new(user_agent: impl Into<String>) -> Result<Self, HttpError> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent.into())
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .build()
            .map_err(|error| HttpError::Network {
                url: String::new(),
                reason: format!("could not build the HTTP client: {error}"),
            })?;
        Ok(Self {
            client,
            initial_backoff: DEFAULT_BACKOFF,
        })
    }

    /// Overrides the first retry delay (it doubles after each attempt).
    pub fn with_backoff(mut self, initial: Duration) -> Self {
        self.initial_backoff = initial;
        self
    }

    /// The underlying client, shared with the download manager.
    pub fn inner(&self) -> &reqwest::Client {
        &self.client
    }

    async fn attempt(&self, url: &str) -> Attempt {
        let network = |error: reqwest::Error| HttpError::Network {
            url: url.to_owned(),
            reason: error.without_url().to_string(),
        };
        let response = match self.client.get(url).send().await {
            Ok(response) => response,
            Err(error) => return Attempt::Retry(network(error), None),
        };
        let status = response.status();
        if status.is_success() {
            return match response.bytes().await {
                Ok(body) => Attempt::Done(Ok(body.to_vec())),
                Err(error) => Attempt::Retry(network(error), None),
            };
        }
        if status == StatusCode::NOT_FOUND {
            return Attempt::Done(Err(HttpError::NotFound(url.to_owned())));
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = parse_retry_after(
                response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|value| value.to_str().ok()),
            );
            let error = HttpError::RateLimited {
                url: url.to_owned(),
                retry_after_secs: retry_after.unwrap_or(0),
            };
            return Attempt::Retry(error, retry_after.map(Duration::from_secs));
        }
        let error = HttpError::Status {
            url: url.to_owned(),
            status: status.as_u16(),
        };
        if status.is_server_error() {
            Attempt::Retry(error, None)
        } else {
            Attempt::Done(Err(error))
        }
    }
}

/// The outcome of one request: final, or retryable after an optional delay.
enum Attempt {
    Done(Result<Vec<u8>, HttpError>),
    Retry(HttpError, Option<Duration>),
}

impl HttpClient for ReqwestHttpClient {
    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let mut backoff = self.initial_backoff;
        let mut attempt = 1;
        loop {
            match self.attempt(url).await {
                Attempt::Done(result) => return result,
                Attempt::Retry(error, _) if attempt >= MAX_ATTEMPTS => return Err(error),
                Attempt::Retry(error, retry_after) => {
                    tracing::debug!(url, attempt, %error, "retrying HTTP request");
                    tokio::time::sleep(retry_after.unwrap_or(backoff)).await;
                    backoff = backoff.saturating_mul(2);
                    attempt += 1;
                }
            }
        }
    }
}

/// `Retry-After` in seconds, capped at 60 s. HTTP dates are ignored.
fn parse_retry_after(value: Option<&str>) -> Option<u64> {
    value?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|secs| secs.min(MAX_RETRY_AFTER_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::http::test_server::{Reply, TestServer};

    fn client() -> ReqwestHttpClient {
        ReqwestHttpClient::new("cinepol-launcher-tests/1.0")
            .unwrap()
            .with_backoff(Duration::from_millis(1))
    }

    #[tokio::test]
    async fn retries_a_server_error_then_returns_the_body() {
        let server = TestServer::start(vec![(
            "/manifest.json",
            vec![Reply::status(500), Reply::ok("{}")],
        )])
        .await;
        let body = client()
            .get_bytes(&server.url("/manifest.json"))
            .await
            .unwrap();
        assert_eq!(body, b"{}");
        assert_eq!(server.hits("/manifest.json"), 2);
    }

    #[tokio::test]
    async fn not_found_fails_without_retrying() {
        let server = TestServer::start(vec![]).await;
        let url = server.url("/missing.json");
        assert_eq!(
            client().get_bytes(&url).await,
            Err(HttpError::NotFound(url.clone()))
        );
        assert_eq!(server.hits("/missing.json"), 1);
    }

    #[tokio::test]
    async fn other_client_errors_fail_without_retrying() {
        let server = TestServer::start(vec![("/forbidden", vec![Reply::status(403)])]).await;
        let url = server.url("/forbidden");
        assert_eq!(
            client().get_bytes(&url).await,
            Err(HttpError::Status { url, status: 403 })
        );
        assert_eq!(server.hits("/forbidden"), 1);
    }

    #[tokio::test]
    async fn persistent_server_errors_give_up_after_three_attempts() {
        let server = TestServer::start(vec![("/down", vec![Reply::status(503)])]).await;
        let url = server.url("/down");
        assert_eq!(
            client().get_bytes(&url).await,
            Err(HttpError::Status { url, status: 503 })
        );
        assert_eq!(server.hits("/down"), 3);
    }

    #[tokio::test]
    async fn exhausted_rate_limits_report_retry_after() {
        let server = TestServer::start(vec![(
            "/limited",
            vec![Reply::status(429).with_header("Retry-After", "0")],
        )])
        .await;
        let url = server.url("/limited");
        assert_eq!(
            client().get_bytes(&url).await,
            Err(HttpError::RateLimited {
                url,
                retry_after_secs: 0
            })
        );
        assert_eq!(server.hits("/limited"), 3);
    }

    #[tokio::test]
    async fn sends_the_user_agent() {
        let server = TestServer::start(vec![("/ua", vec![Reply::ok("ok")])]).await;
        client().get_bytes(&server.url("/ua")).await.unwrap();
        let requests = server.requests();
        assert_eq!(
            requests[0].headers.get("user-agent").map(String::as_str),
            Some("cinepol-launcher-tests/1.0")
        );
    }

    #[test]
    fn retry_after_is_capped_at_sixty_seconds() {
        assert_eq!(parse_retry_after(Some("120")), Some(60));
        assert_eq!(parse_retry_after(Some(" 7 ")), Some(7));
        assert_eq!(
            parse_retry_after(Some("Wed, 21 Oct 2026 07:28:00 GMT")),
            None
        );
        assert_eq!(parse_retry_after(None), None);
    }
}
