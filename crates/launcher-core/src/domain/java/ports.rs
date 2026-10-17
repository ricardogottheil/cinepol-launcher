use std::future::Future;
use std::path::Path;

use thiserror::Error;

use super::JavaVersion;

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
