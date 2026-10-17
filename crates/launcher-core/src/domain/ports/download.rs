use std::future::Future;
use std::path::PathBuf;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use super::ProgressReporter;
use crate::domain::integrity::Checksum;

/// One file to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub url: String,
    pub destination: PathBuf,
    /// When present, the file is verified and a mismatch fails the download.
    pub checksum: Option<Checksum>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DownloadSummary {
    /// Files fetched from the network.
    pub downloaded: usize,
    /// Files already present with a matching checksum (or size, without one).
    pub reused: usize,
    /// Bytes fetched from the network.
    pub bytes: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DownloadError {
    #[error("download of {url} failed: {reason}")]
    Http { url: String, reason: String },
    #[error("{url} does not match its checksum (expected {expected}, got {actual})")]
    ChecksumMismatch {
        url: String,
        expected: String,
        actual: String,
    },
    #[error("could not write {path}: {reason}")]
    Io { path: String, reason: String },
    #[error("the download was cancelled")]
    Cancelled,
}

/// Fetches many files with bounded concurrency, retries with backoff,
/// verification, atomic writes (temporary file + rename) and reuse of files
/// that are already valid. Progress is reported in bytes over the whole batch.
pub trait Downloader: Send + Sync {
    fn download_all<R: ProgressReporter>(
        &self,
        requests: Vec<DownloadRequest>,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> impl Future<Output = Result<DownloadSummary, DownloadError>> + Send;
}
