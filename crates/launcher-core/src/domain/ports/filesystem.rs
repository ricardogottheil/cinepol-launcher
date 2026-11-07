use std::future::Future;
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("{path}: {reason}")]
pub struct FileSystemError {
    pub path: String,
    pub reason: String,
}

/// How much of a text file to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCap {
    /// The first bytes (where a crash report says what failed).
    Head(usize),
    /// The last bytes (where a log ends).
    Tail(usize),
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
    /// The newest regular file directly inside `dir` whose name starts with
    /// `name_prefix` (`""` for any) and that was modified at or after `since`
    /// (crash reports written during a launch); `None` when there is none or
    /// the folder is missing. Entries that cannot be read are skipped; only
    /// an unreadable folder is an error.
    fn newest_file_since(
        &self,
        dir: &Path,
        since: std::time::SystemTime,
        name_prefix: &str,
    ) -> impl Future<Output = Result<Option<std::path::PathBuf>, FileSystemError>> + Send;
    /// Up to `cap` bytes of a regular file as text (invalid UTF-8 replaced);
    /// `None` when it is missing or not a regular file (a symbolic link is not).
    /// When the cap cuts through a line, that partial line is dropped, so a
    /// secret on it is never returned in pieces.
    fn read_text(
        &self,
        path: &Path,
        cap: TextCap,
    ) -> impl Future<Output = Result<Option<String>, FileSystemError>> + Send;
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
