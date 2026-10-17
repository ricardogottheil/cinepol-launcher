use std::future::Future;
use std::path::Path;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use super::{JavaRequirement, JavaRuntime, JavaVersion};
use crate::domain::launch::Platform;
use crate::domain::ports::ProgressReporter;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum JavaProbeError {
    #[error("{0} is not an executable Java")]
    NotExecutable(String),
    #[error("could not read the Java version: {0}")]
    Unreadable(String),
}

/// Runs `<java> -version` and parses the result.
pub trait JavaProbe: Send + Sync {
    fn probe(
        &self,
        executable: &Path,
    ) -> impl Future<Output = Result<JavaVersion, JavaProbeError>> + Send;
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum JavaRuntimeError {
    #[error("no Java {major} runtime is available for {platform}")]
    Unavailable { major: u32, platform: String },
    #[error("downloading the Java runtime failed: {0}")]
    Download(String),
    #[error("the installed Java runtime is broken: {0}")]
    Invalid(String),
    #[error("the Java installation was cancelled")]
    Cancelled,
}

/// Makes a managed runtime for `requirement` available: Mojang's runtime
/// manifest first, Adoptium (Temurin) when Mojang has no build for the platform.
/// Reuses an already installed and verified runtime.
pub trait JavaRuntimeProvider: Send + Sync {
    fn ensure<R: ProgressReporter>(
        &self,
        requirement: &JavaRequirement,
        platform: &Platform,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> impl Future<Output = Result<JavaRuntime, JavaRuntimeError>> + Send;
}
