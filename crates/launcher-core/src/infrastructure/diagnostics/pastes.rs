//! Publishes reports on pastes.dev, the paste site of the Minecraft
//! community (https://github.com/lucko/paste/blob/master/API.md). Unlike
//! mclo.gs, it keeps IP addresses, which hosts and ISPs need to read a path.

use serde::Deserialize;

use crate::domain::diagnostics::{ReportHost, ReportShareError, SharedReport};
use crate::domain::ports::{HttpBody, HttpRequest, HttpRequester};

pub const PASTES_API_URL: &str = "https://api.pastes.dev/post";
/// Where a paste is read: `<VIEW_URL><key>`.
pub const PASTES_VIEW_URL: &str = "https://pastes.dev/";
/// Keys are short alphanumeric ids; anything else is not trusted in a link.
const MAX_KEY_LEN: usize = 64;

#[derive(Deserialize)]
struct CreatedPaste {
    #[serde(default)]
    key: Option<String>,
}

fn is_valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_KEY_LEN && key.bytes().all(|b| b.is_ascii_alphanumeric())
}

pub struct PastesReportHost<R> {
    http: R,
    url: String,
}

impl<R: HttpRequester> PastesReportHost<R> {
    pub fn new(http: R) -> Self {
        Self::with_url(http, PASTES_API_URL)
    }

    pub fn with_url(http: R, url: impl Into<String>) -> Self {
        Self {
            http,
            url: url.into(),
        }
    }
}

impl<R: HttpRequester> ReportHost for PastesReportHost<R> {
    /// One upload per player click, as the API's terms ask: never retried.
    async fn publish(&self, content: &str) -> Result<SharedReport, ReportShareError> {
        let body = HttpBody::Text {
            content_type: "text/plain",
            data: content.to_owned(),
        };
        let reply = self
            .http
            .send(HttpRequest::post(&self.url, body))
            .await
            .map_err(|error| ReportShareError::Unavailable(error.to_string()))?;
        if (400..500).contains(&reply.status) {
            return Err(ReportShareError::Refused(format!(
                "pastes.dev answered {}",
                reply.status
            )));
        }
        if !reply.is_success() {
            return Err(ReportShareError::Unavailable(format!(
                "pastes.dev answered {}",
                reply.status
            )));
        }
        let created: CreatedPaste = serde_json::from_slice(&reply.body).map_err(|error| {
            ReportShareError::Unavailable(format!("unexpected answer: {error}"))
        })?;
        match created.key {
            Some(key) if is_valid_key(&key) => Ok(SharedReport {
                url: format!("{PASTES_VIEW_URL}{key}"),
                expires_at: None,
            }),
            _ => Err(ReportShareError::Unavailable(
                "the answer has no valid key".to_owned(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHttpRequester;

    const CREATED: &str = include_str!("../../../tests/fixtures/pastes/created.json");

    fn host(status: u16, answer: &str) -> PastesReportHost<FakeHttpRequester> {
        PastesReportHost::new(FakeHttpRequester::default().reply(PASTES_API_URL, status, answer))
    }

    #[tokio::test]
    async fn posts_the_text_unchanged_and_returns_the_link() {
        let host = host(201, CREATED);
        let report = "Network path report\n 8  103.195.101.252  Server";
        let shared = host.publish(report).await.unwrap();
        assert_eq!(shared.url, "https://pastes.dev/aB3dE9fGh2");
        assert_eq!(shared.expires_at, None);
        let requests = host.http.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, PASTES_API_URL);
        assert_eq!(
            requests[0].body,
            Some(HttpBody::Text {
                content_type: "text/plain",
                data: report.to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn a_client_error_is_a_refusal() {
        assert!(matches!(
            host(413, "").publish("x").await,
            Err(ReportShareError::Refused(_))
        ));
    }

    #[tokio::test]
    async fn a_server_error_or_no_answer_is_unavailable() {
        assert!(matches!(
            host(503, "").publish("x").await,
            Err(ReportShareError::Unavailable(_))
        ));
        let silent = PastesReportHost::new(FakeHttpRequester::default());
        assert!(matches!(
            silent.publish("x").await,
            Err(ReportShareError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn a_key_that_could_change_the_link_is_rejected() {
        for answer in [
            r#"{"key":"../evil"}"#,
            r#"{"key":"a b"}"#,
            r#"{"key":""}"#,
            r#"{}"#,
        ] {
            assert!(
                matches!(
                    host(201, answer).publish("x").await,
                    Err(ReportShareError::Unavailable(_))
                ),
                "{answer}"
            );
        }
    }
}
