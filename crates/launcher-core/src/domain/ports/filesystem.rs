use std::future::Future;
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("{path}: {reason}")]
pub struct FileSystemError {
    pub path: String,
    pub reason: String,
}

/// The few file-system operations use cases need directly; everything else
/// (downloads, extraction, repositories) goes through its own port.
pub trait FileSystem: Send + Sync {
    fn exists(&self, path: &Path) -> impl Future<Output = bool> + Send;
    fn create_dir_all(
        &self,
        path: &Path,
    ) -> impl Future<Output = Result<(), FileSystemError>> + Send;
    /// Removes a directory tree; a missing directory is not an error.
    fn remove_dir_all(
        &self,
        path: &Path,
    ) -> impl Future<Output = Result<(), FileSystemError>> + Send;
}
