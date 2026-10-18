//! `HttpClient` over reqwest, with retries for transient failures, and the
//! single-shot `HttpRequester` used by the sign-in exchanges.

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue, RETRY_AFTER};

use crate::domain::ports::{
    HttpBody, HttpClient, HttpError, HttpMethod, HttpRequest, HttpRequester, HttpResponse, Secret,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ATTEMPTS: u32 = 3;
const DEFAULT_BACKOFF: Duration = Duration::from_millis(500);
const MAX_RETRY_AFTER_SECS: u64 = 60;

/// Sends the launcher's User-Agent and retries network errors, HTTP 5xx and
/// 429 up to three attempts with exponential backoff (waiting `Retry-After`
/// on 429), for GETs and JSON POSTs alike.
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

    /// Sends a GET, or a JSON POST when `json` is set, with retries.
    async fn send_with_retries(
        &self,
        url: &str,
        json: Option<&[u8]>,
    ) -> Result<Vec<u8>, HttpError> {
        let mut backoff = self.initial_backoff;
        let mut attempt = 1;
        loop {
            match self.attempt(url, json).await {
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

    async fn attempt(&self, url: &str, json: Option<&[u8]>) -> Attempt {
        let network = |error: reqwest::Error| HttpError::Network {
            url: url.to_owned(),
            reason: error.without_url().to_string(),
        };
        let request = match json {
            None => self.client.get(url),
            Some(body) => self
                .client
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json")
                .body(body.to_vec()),
        };
        let response = match request.send().await {
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
        self.send_with_retries(url, None).await
    }

    async fn post_json(&self, url: &str, body: Vec<u8>) -> Result<Vec<u8>, HttpError> {
        self.send_with_retries(url, Some(&body)).await
    }
}

/// Sign-in answers are small JSON documents; anything larger is refused.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Sent once, never retried. Request bodies and bearer tokens are never
/// logged, and error messages carry the URL without its query string.
impl HttpRequester for ReqwestHttpClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let HttpRequest {
            method,
            url,
            body,
            bearer,
        } = request;
        let shown_url = without_query(&url).to_owned();
        let network = |reason: String| HttpError::Network {
            url: shown_url.clone(),
            reason,
        };
        let mut builder = match method {
            HttpMethod::Get => self.client.get(&url),
            HttpMethod::Post => self.client.post(&url),
        };
        if let Some(token) = bearer {
            let mut value = HeaderValue::from_str(&format!("Bearer {}", token.expose()))
                .map_err(|_| network("the bearer token is not a valid header value".into()))?;
            value.set_sensitive(true);
            builder = builder.header(AUTHORIZATION, value);
        }
        builder = match body {
            None => builder,
            Some(HttpBody::Form(fields)) => builder
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(encode_form(&fields)),
            Some(HttpBody::Json(document)) => builder
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json")
                .body(document.expose().to_owned()),
        };
        let mut response = builder
            .send()
            .await
            .map_err(|error| network(error.without_url().to_string()))?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| network(error.without_url().to_string()))?
        {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(network(format!(
                    "the answer is larger than {MAX_RESPONSE_BYTES} bytes"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse { status, body })
    }
}

fn without_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// `application/x-www-form-urlencoded`: unreserved bytes stay, a space
/// becomes `+`, everything else is percent-encoded.
fn encode_form(fields: &[(String, Secret)]) -> String {
    fields
        .iter()
        .map(|(name, value)| {
            format!(
                "{}={}",
                encode_component(name),
                encode_component(value.expose())
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn encode_component(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'*' => {
                encoded.push(char::from(byte));
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
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

    #[tokio::test]
    async fn posts_json_with_the_user_agent_and_retries_server_errors() {
        let server = TestServer::start(vec![(
            "/version_files",
            vec![Reply::status(502), Reply::ok(r#"{"a":1}"#)],
        )])
        .await;
        let body = client()
            .post_json(&server.url("/version_files"), br#"{"hashes":[]}"#.to_vec())
            .await
            .unwrap();
        assert_eq!(body, br#"{"a":1}"#);
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            assert_eq!(request.method, "POST");
            assert_eq!(request.body, br#"{"hashes":[]}"#);
            assert_eq!(
                request.headers.get("content-type").map(String::as_str),
                Some("application/json")
            );
            assert_eq!(
                request.headers.get("user-agent").map(String::as_str),
                Some("cinepol-launcher-tests/1.0")
            );
        }
    }

    #[tokio::test]
    async fn posts_honour_rate_limits_like_gets() {
        let server = TestServer::start(vec![(
            "/limited",
            vec![Reply::status(429).with_header("Retry-After", "0")],
        )])
        .await;
        let url = server.url("/limited");
        assert_eq!(
            client().post_json(&url, b"{}".to_vec()).await,
            Err(HttpError::RateLimited {
                url,
                retry_after_secs: 0
            })
        );
        assert_eq!(server.hits("/limited"), 3);
    }

    #[tokio::test]
    async fn posts_fail_on_client_errors_without_retrying() {
        let server = TestServer::start(vec![("/bad", vec![Reply::status(400)])]).await;
        let url = server.url("/bad");
        assert_eq!(
            client().post_json(&url, b"{}".to_vec()).await,
            Err(HttpError::Status { url, status: 400 })
        );
        assert_eq!(server.hits("/bad"), 1);
    }

    fn form(fields: &[(&str, &str)]) -> HttpBody {
        HttpBody::Form(
            fields
                .iter()
                .map(|(name, value)| ((*name).to_owned(), Secret::new(*value)))
                .collect(),
        )
    }

    #[tokio::test]
    async fn posts_an_urlencoded_form_once_and_returns_an_error_status_with_its_body() {
        let server = TestServer::start(vec![(
            "/token",
            vec![Reply::status(400).with_body(r#"{"error":"authorization_pending"}"#)],
        )])
        .await;
        let response = client()
            .send(HttpRequest::post(
                server.url("/token"),
                form(&[("grant_type", "urn:ietf:params"), ("scope", "a b&c=d")]),
            ))
            .await
            .unwrap();
        assert_eq!(response.status, 400);
        assert_eq!(response.body, br#"{"error":"authorization_pending"}"#);
        let requests = server.requests();
        assert_eq!(requests.len(), 1, "never retried");
        assert_eq!(requests[0].method, "POST");
        assert_eq!(
            requests[0].headers.get("content-type").map(String::as_str),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(
            String::from_utf8_lossy(&requests[0].body),
            "grant_type=urn%3Aietf%3Aparams&scope=a+b%26c%3Dd"
        );
        assert_eq!(
            requests[0].headers.get("user-agent").map(String::as_str),
            Some("cinepol-launcher-tests/1.0")
        );
    }

    #[tokio::test]
    async fn server_errors_are_returned_without_retrying() {
        let server = TestServer::start(vec![("/down", vec![Reply::status(503)])]).await;
        let response = client()
            .send(HttpRequest::get(server.url("/down")))
            .await
            .unwrap();
        assert_eq!(response.status, 503);
        assert_eq!(server.hits("/down"), 1);
    }

    #[tokio::test]
    async fn posts_json_with_its_headers() {
        let server = TestServer::start(vec![("/xsts", vec![Reply::ok(r#"{"Token":"t"}"#)])]).await;
        let response = client()
            .send(HttpRequest::post(
                server.url("/xsts"),
                HttpBody::Json(Secret::new(r#"{"TokenType":"JWT"}"#)),
            ))
            .await
            .unwrap();
        assert!(response.is_success());
        assert_eq!(response.body, br#"{"Token":"t"}"#);
        let request = &server.requests()[0];
        assert_eq!(request.body, br#"{"TokenType":"JWT"}"#);
        assert_eq!(
            request.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(
            request.headers.get("accept").map(String::as_str),
            Some("application/json")
        );
    }

    #[tokio::test]
    async fn sends_the_bearer_in_the_authorization_header() {
        let server = TestServer::start(vec![("/profile", vec![Reply::ok("{}")])]).await;
        client()
            .send(HttpRequest::get(server.url("/profile")).with_bearer(&Secret::new("mc-token")))
            .await
            .unwrap();
        let request = &server.requests()[0];
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer mc-token")
        );
    }

    #[tokio::test]
    async fn refuses_bodies_over_one_mebibyte() {
        let big = vec![b'a'; MAX_RESPONSE_BYTES + 1];
        let server = TestServer::start(vec![("/big", vec![Reply::ok(big)])]).await;
        let result = client().send(HttpRequest::get(server.url("/big"))).await;
        assert!(
            matches!(result, Err(HttpError::Network { .. })),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn transport_failures_are_network_errors_without_the_query() {
        // Nothing listens on a port that was bound and released again.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let result = client()
            .send(HttpRequest::get(format!(
                "http://{addr}/x?code=secret-code"
            )))
            .await;
        match result {
            Err(error @ HttpError::Network { .. }) => {
                assert!(!error.to_string().contains("secret-code"), "{error}");
            }
            other => panic!("expected a network error, got {other:?}"),
        }
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
