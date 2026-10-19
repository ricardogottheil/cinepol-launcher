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
    /// Copies the directory tree `from` into the new directory `to`. Symbolic
    /// links are skipped, so nothing outside `from` is copied; an existing
    /// `to` is an error, never merged or replaced. When the copy fails after
    /// `to` was created, `to` is removed again, and only then: a folder that
    /// existed before is never touched.
    fn copy_dir_all(
        &self,
        from: &Path,
        to: &Path,
    ) -> impl Future<Output = Result<(), FileSystemError>> + Send;
}
