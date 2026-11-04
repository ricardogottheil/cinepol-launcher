//! Publishes reports on mclo.gs, the Minecraft log paste site
//! (https://api.mclo.gs/).

use serde::{Deserialize, Serialize};

use crate::domain::diagnostics::{ReportHost, ReportShareError, SharedReport};
use crate::domain::ports::{HttpClient, HttpError};

pub const MCLOGS_API_URL: &str = "https://api.mclo.gs/1/log";
/// Shown on the paste page as where the log came from.
const SOURCE: &str = "Cinepol Launcher";

#[derive(Serialize)]
struct CreateLog<'a> {
    content: &'a str,
    source: &'a str,
}

#[derive(Deserialize)]
struct CreatedLog {
    success: bool,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    expires: Option<u64>,
    #[serde(default)]
    error: Option<String>,
}

pub struct MclogsReportHost<C> {
    http: C,
    url: String,
}

impl<C: HttpClient> MclogsReportHost<C> {
    pub fn new(http: C) -> Self {
        Self::with_url(http, MCLOGS_API_URL)
    }

    pub fn with_url(http: C, url: impl Into<String>) -> Self {
        Self {
            http,
            url: url.into(),
        }
    }
}

impl<C: HttpClient> ReportHost for MclogsReportHost<C> {
    async fn publish(&self, content: &str) -> Result<SharedReport, ReportShareError> {
        let body = serde_json::to_vec(&CreateLog {
            content,
            source: SOURCE,
        })
        .map_err(|error| ReportShareError::Unavailable(error.to_string()))?;
        let reply = self
            .http
            .post_json(&self.url, body)
            .await
            .map_err(|error| match error {
                HttpError::Status { status, .. } if (400..500).contains(&status) => {
                    ReportShareError::Refused(error.to_string())
                }
                other => ReportShareError::Unavailable(other.to_string()),
            })?;
        let created: CreatedLog = serde_json::from_slice(&reply).map_err(|error| {
            ReportShareError::Unavailable(format!("unexpected answer: {error}"))
        })?;
        match (created.success, created.url) {
            (true, Some(url)) if url.starts_with("https://") => Ok(SharedReport {
                url,
                expires_at: created.expires,
            }),
            _ => Err(ReportShareError::Refused(
                created
                    .error
                    .unwrap_or_else(|| "the answer has no link".to_owned()),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHttpClient;

    const CREATED: &str = include_str!("../../../tests/fixtures/mclogs/created.json");

    fn host(answer: &str) -> MclogsReportHost<FakeHttpClient> {
        MclogsReportHost::new(FakeHttpClient::with([(
            MCLOGS_API_URL.to_owned(),
            answer.as_bytes().to_vec(),
        )]))
    }

    #[tokio::test]
    async fn posts_the_content_and_returns_the_link() {
        let host = host(CREATED);
        let shared = host.publish("Network diagnosis\nhop 1").await.unwrap();
        assert_eq!(shared.url, "https://mclo.gs/WnMMikq");
        assert_eq!(shared.expires_at, Some(1_777_373_979));
        let posts = host.http.posts();
        assert_eq!(posts.len(), 1);
        let sent: serde_json::Value = serde_json::from_slice(&posts[0].1).unwrap();
        assert_eq!(sent["content"], "Network diagnosis\nhop 1");
        assert_eq!(sent["source"], "Cinepol Launcher");
    }

    #[tokio::test]
    async fn a_refusal_carries_its_reason() {
        let host = host(r#"{"success":false,"error":"Required field 'content' not found."}"#);
        assert_eq!(
            host.publish("x").await,
            Err(ReportShareError::Refused(
                "Required field 'content' not found.".into()
            ))
        );
    }

    #[tokio::test]
    async fn an_unreachable_host_is_unavailable() {
        let host = MclogsReportHost::new(FakeHttpClient::default());
        assert!(matches!(
            host.publish("x").await,
            Err(ReportShareError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn a_link_that_is_not_https_is_refused() {
        let host = host(r#"{"success":true,"url":"javascript:alert(1)"}"#);
        assert!(matches!(
            host.publish("x").await,
            Err(ReportShareError::Refused(_))
        ));
    }
}
