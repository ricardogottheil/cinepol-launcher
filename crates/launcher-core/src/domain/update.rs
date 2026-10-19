//! Updates of the launcher itself (not of game instances).

use std::future::Future;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use super::ports::ProgressReporter;

/// A newer launcher release the update source announces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdate {
    pub version: String,
    pub current_version: String,
    /// Release notes, when the release has them.
    pub notes: Option<String>,
}

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// This build has no update source or signing key (development builds).
    NotConfigured,
    UpToDate,
    Available(AppUpdate),
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum AppUpdateError {
    #[error("updates are not configured in this build")]
    NotConfigured,
    #[error("could not check for updates: {0}")]
    Check(String),
    /// The release changed, or there is none any more, since it was shown.
    #[error("version {0} is no longer the available update")]
    Changed(String),
    /// Downloading failed or the signature did not verify.
    #[error("could not download the update: {0}")]
    Download(String),
    #[error("could not install the update: {0}")]
    Install(String),
    /// Installing restarts the launcher, which would cut a game session short.
    #[error("the update cannot be installed while a game is installing or running")]
    GameRunning,
    #[error("the update was cancelled")]
    Cancelled,
}

/// The launcher's update source. Downloads are verified against the build's
/// signing key before `apply` can install them.
pub trait AppUpdater: Send + Sync {
    /// `false` when the build has no update source or key.
    fn is_configured(&self) -> bool;

    fn check(&self) -> impl Future<Output = Result<Option<AppUpdate>, AppUpdateError>> + Send;

    /// Downloads and verifies `version`, refusing any other version (the
    /// source may have moved on since the player saw it).
    fn download<R: ProgressReporter>(
        &self,
        version: &str,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> impl Future<Output = Result<(), AppUpdateError>> + Send;

    /// Installs the last verified download. The launcher restarts afterwards.
    fn apply(&self) -> impl Future<Output = Result<(), AppUpdateError>> + Send;
}
