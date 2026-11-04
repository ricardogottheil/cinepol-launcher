//! Publishes a diagnosis report the player wrote out (as plain text) and
//! returns a link they can hand to the server's staff.

use thiserror::Error;

use crate::domain::diagnostics::{ReportHost, ReportShareError, SharedReport};

/// Reports are a few kilobytes; anything far larger is not one.
pub const MAX_REPORT_BYTES: usize = 512 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ShareReportError {
    #[error("the report is empty")]
    Empty,
    #[error("the report is too large ({0} bytes)")]
    TooLarge(usize),
    #[error(transparent)]
    Host(#[from] ReportShareError),
}

pub async fn share_diagnostic_report<H: ReportHost>(
    content: &str,
    host: &H,
) -> Result<SharedReport, ShareReportError> {
    if content.trim().is_empty() {
        return Err(ShareReportError::Empty);
    }
    if content.len() > MAX_REPORT_BYTES {
        return Err(ShareReportError::TooLarge(content.len()));
    }
    Ok(host.publish(content).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeReportHost;

    fn link() -> SharedReport {
        SharedReport {
            url: "https://mclo.gs/WnMMikq".into(),
            expires_at: Some(1_777_373_979),
        }
    }

    #[tokio::test]
    async fn publishes_the_report_as_written() {
        let host = FakeReportHost::answering(Ok(link()));
        assert_eq!(
            share_diagnostic_report("Network diagnosis\n", &host).await,
            Ok(link())
        );
        assert_eq!(host.published(), ["Network diagnosis\n"]);
    }

    #[tokio::test]
    async fn refuses_empty_and_oversized_reports_without_uploading() {
        let host = FakeReportHost::answering(Ok(link()));
        assert_eq!(
            share_diagnostic_report(" \n", &host).await,
            Err(ShareReportError::Empty)
        );
        let huge = "x".repeat(MAX_REPORT_BYTES + 1);
        assert_eq!(
            share_diagnostic_report(&huge, &host).await,
            Err(ShareReportError::TooLarge(MAX_REPORT_BYTES + 1))
        );
        assert!(host.published().is_empty());
    }

    #[tokio::test]
    async fn passes_host_failures_on() {
        let host = FakeReportHost::answering(Err(ReportShareError::Unavailable("offline".into())));
        assert_eq!(
            share_diagnostic_report("report", &host).await,
            Err(ShareReportError::Host(ReportShareError::Unavailable(
                "offline".into()
            )))
        );
    }
}
