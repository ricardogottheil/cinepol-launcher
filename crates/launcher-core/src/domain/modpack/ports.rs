use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::mrpack::{MrpackError, MrpackIndex};
use super::sync::{LocalFile, PackManifest};
use super::{ModpackPage, ModpackQuery, ModpackSourceKind, ModpackSummary, ModpackVersion};
use crate::domain::instance::InstanceId;
use crate::domain::launch::RelativePath;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModpackSourceError {
    #[error("{0:?} is not configured (missing API key)")]
    NotConfigured(ModpackSourceKind),
    #[error("modpack {0} was not found")]
    NotFound(String),
    #[error("rate limited; retry after {retry_after_secs} s")]
    RateLimited { retry_after_secs: u64 },
    #[error("modpack request failed: {0}")]
    Network(String),
    #[error("unexpected response: {0}")]
    InvalidResponse(String),
}

/// A browsable modpack catalog. Installing a version is a separate use case
/// built on top (Milestones 10-12).
pub trait ModpackSource: Send + Sync {
    fn kind(&self) -> ModpackSourceKind;
    fn search(
        &self,
        query: &ModpackQuery,
    ) -> impl Future<Output = Result<ModpackPage, ModpackSourceError>> + Send;
    /// One project's details.
    fn project(
        &self,
        project: &str,
    ) -> impl Future<Output = Result<ModpackSummary, ModpackSourceError>> + Send;
    /// A project's versions, newest first.
    fn versions(
        &self,
        project: &str,
    ) -> impl Future<Output = Result<Vec<ModpackVersion>, ModpackSourceError>> + Send;
    /// Download URLs of the files the catalog knows, keyed by the sha512
    /// (lowercase hex) it was asked about; unknown hashes are left out.
    fn known_files(
        &self,
        sha512: &[String],
    ) -> impl Future<Output = Result<HashMap<String, Vec<String>>, ModpackSourceError>> + Send;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PackFileError {
    #[error("{path}: {reason}")]
    Io { path: String, reason: String },
    #[error("not a valid modpack archive: {0}")]
    InvalidArchive(String),
    #[error("archive entry escapes its folder: {0}")]
    UnsafeEntry(String),
    #[error(transparent)]
    Index(#[from] MrpackError),
}

/// A file extracted from an archive into a staging folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedFile {
    /// Where it goes, relative to the game folder.
    pub path: RelativePath,
    /// Lowercase hex sha1 of its content.
    pub sha1: String,
    /// Where it was extracted.
    pub staged: PathBuf,
}

/// Reads and writes `.mrpack` archives.
pub trait MrpackArchive: Send + Sync {
    /// Reads and validates `modrinth.index.json` (size-capped).
    fn read_index(
        &self,
        archive: &Path,
    ) -> impl Future<Output = Result<MrpackIndex, PackFileError>> + Send;
    /// Extracts `overrides/` and then `client-overrides/` into `staging`,
    /// stripping the prefix; a client override replaces an override at the
    /// same path. `server-overrides/` is never extracted. Entries that would
    /// escape `staging` fail with [`PackFileError::UnsafeEntry`].
    fn extract_overrides(
        &self,
        archive: &Path,
        staging: &Path,
    ) -> impl Future<Output = Result<Vec<StagedFile>, PackFileError>> + Send;
    /// Writes a pack: the index plus each override, copied from its source
    /// file into `overrides/<path>`. The archive is written atomically.
    fn write(
        &self,
        archive: &Path,
        index: &MrpackIndex,
        overrides: &[(RelativePath, PathBuf)],
    ) -> impl Future<Output = Result<(), PackFileError>> + Send;
}

/// File operations on an instance's game folder.
pub trait PackFiles: Send + Sync {
    /// Lowercase hex sha1 of a file, `None` when it does not exist.
    fn sha1(
        &self,
        path: &Path,
    ) -> impl Future<Output = Result<Option<String>, PackFileError>> + Send;
    /// Moves (or copies) `from` to `to`, creating parent folders and replacing
    /// `to` atomically.
    fn place(
        &self,
        from: &Path,
        to: &Path,
    ) -> impl Future<Output = Result<(), PackFileError>> + Send;
    /// Deletes one file; a missing file is not an error.
    fn remove_file(&self, path: &Path) -> impl Future<Output = Result<(), PackFileError>> + Send;
    /// Every regular file under `root/<entry>` for each entry (a folder or a
    /// file), with its path relative to `root` and its digests. Symbolic
    /// links and missing entries are skipped.
    fn scan(
        &self,
        root: &Path,
        entries: &[&str],
    ) -> impl Future<Output = Result<Vec<LocalFile>, PackFileError>> + Send;
}

/// The per-instance record of the files a pack installed.
pub trait PackManifestStore: Send + Sync {
    /// The saved manifest, or an empty one when the instance has none.
    fn load(
        &self,
        id: &InstanceId,
    ) -> impl Future<Output = Result<PackManifest, PackFileError>> + Send;
    fn save(
        &self,
        id: &InstanceId,
        manifest: &PackManifest,
    ) -> impl Future<Output = Result<(), PackFileError>> + Send;
}
