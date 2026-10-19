//! packwiz packs read over HTTP (`https` URL of a `pack.toml`) or from a
//! local folder.
//!
//! Every file is resolved relative to the folder holding `pack.toml` and can
//! never leave it: remote paths are appended segment by segment
//! (percent-encoded), and local paths are checked after resolving symbolic
//! links. Reads are size-capped, and files the index lists are verified
//! against the expected checksum before they are parsed.

mod wire;

use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use sha2::Digest;

use crate::domain::integrity::{Checksum, HashAlgorithm, curseforge_fingerprint};
use crate::domain::launch::RelativePath;
use crate::domain::modpack::packwiz::{PackwizError, RawMetafile, RawPackIndex, RawPackToml};
use crate::domain::modpack::{PackFileError, PackFileLocation, PackLocation, PackwizSource};
use crate::domain::ports::{HttpClient, HttpError};

pub use wire::{parse_index, parse_metafile, parse_pack};

/// Largest `pack.toml` or `*.pw.toml` accepted (1 MiB). Real ones are well
/// under 4 KiB.
const METAFILE_LIMIT: usize = 1024 * 1024;
/// Largest `index.toml` accepted (16 MiB). The Fabulously Optimized index,
/// 108 entries, is about 16 KiB, so this leaves room for packs with
/// thousands of files.
const INDEX_LIMIT: usize = 16 * 1024 * 1024;
const PACK_FILE: &str = "pack.toml";

/// `PackwizSource` over the `HttpClient` port for URLs and the local file
/// system for paths.
#[derive(Debug, Clone)]
pub struct PackwizFiles<H> {
    http: H,
}

impl<H> PackwizFiles<H> {
    pub fn new(http: H) -> Self {
        Self { http }
    }
}

impl<H: HttpClient> PackwizFiles<H> {
    /// The bytes of `pack.toml` itself, capped at `limit`.
    async fn fetch_pack(&self, pack: &PackLocation) -> Result<Vec<u8>, PackFileError> {
        match pack {
            PackLocation::Url(url) => {
                let url = https_url(url)?;
                self.get(url.as_str(), PACK_FILE, METAFILE_LIMIT).await
            }
            PackLocation::Local(path) => {
                let file = local_pack_file(path);
                read_capped(file, PACK_FILE.to_owned(), METAFILE_LIMIT).await
            }
        }
    }

    /// The bytes of a file relative to the pack folder, capped at `limit`
    /// and verified against `expected`.
    async fn fetch_verified(
        &self,
        pack: &PackLocation,
        relative: &RelativePath,
        expected: &Checksum,
        limit: usize,
    ) -> Result<Vec<u8>, PackFileError> {
        let bytes = match self.locate(pack, relative)? {
            PackFileLocation::Url(url) => self.get(&url, relative.as_str(), limit).await?,
            PackFileLocation::Path(path) => {
                read_capped(path, relative.as_str().to_owned(), limit).await?
            }
        };
        verify(relative.as_str(), &bytes, expected)?;
        Ok(bytes)
    }

    async fn get(&self, url: &str, name: &str, limit: usize) -> Result<Vec<u8>, PackFileError> {
        // The cap is applied while reading, so a huge answer is never buffered.
        self.http
            .get_bytes_limited(url, limit)
            .await
            .map_err(|error| match error {
                HttpError::TooLarge { .. } => PackFileError::TooLarge(name.to_owned()),
                other => PackFileError::Source(other.to_string()),
            })
    }
}

impl<H: HttpClient> PackwizSource for PackwizFiles<H> {
    async fn read_pack(&self, pack: &PackLocation) -> Result<RawPackToml, PackFileError> {
        let bytes = self.fetch_pack(pack).await?;
        parse_pack(PACK_FILE, &bytes)
    }

    async fn read_index(
        &self,
        pack: &PackLocation,
        relative: &RelativePath,
        expected: &Checksum,
    ) -> Result<RawPackIndex, PackFileError> {
        let bytes = self
            .fetch_verified(pack, relative, expected, INDEX_LIMIT)
            .await?;
        parse_index(relative.as_str(), &bytes)
    }

    async fn read_metafile(
        &self,
        pack: &PackLocation,
        relative: &RelativePath,
        expected: &Checksum,
    ) -> Result<RawMetafile, PackFileError> {
        let bytes = self
            .fetch_verified(pack, relative, expected, METAFILE_LIMIT)
            .await?;
        parse_metafile(relative.as_str(), &bytes)
    }

    fn locate(
        &self,
        pack: &PackLocation,
        relative: &RelativePath,
    ) -> Result<PackFileLocation, PackFileError> {
        match pack {
            PackLocation::Url(url) => join_url(url, relative).map(PackFileLocation::Url),
            PackLocation::Local(path) => {
                let file = local_pack_file(path);
                let folder = file
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                resolve_inside(folder, relative).map(PackFileLocation::Path)
            }
        }
    }
}

/// A local pack is its `pack.toml`, or a folder holding one.
fn local_pack_file(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(PACK_FILE)
    } else {
        path.to_path_buf()
    }
}

fn https_url(raw: &str) -> Result<reqwest::Url, PackFileError> {
    let url = reqwest::Url::parse(raw).map_err(|error| {
        PackFileError::InvalidLocation(format!("invalid pack URL {raw:?}: {error}"))
    })?;
    if url.scheme() != "https" || url.host_str().is_none_or(str::is_empty) {
        return Err(PackFileError::InvalidLocation(format!(
            "only https URLs are supported: {raw}"
        )));
    }
    Ok(url)
}

/// `relative` resolved against the folder of the `pack.toml` URL `pack`.
///
/// The pack URL's query and fragment belong to `pack.toml` only and are
/// dropped (as packwiz-installer does when it resolves relative URIs). Each
/// segment of `relative` is percent-encoded, so spaces, `?`, `#` and `%` in
/// pack paths stay part of the path and cannot change the URL's meaning.
fn join_url(pack: &str, relative: &RelativePath) -> Result<String, PackFileError> {
    let mut url = https_url(pack)?;
    url.set_query(None);
    url.set_fragment(None);
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| PackFileError::InvalidLocation(format!("invalid pack URL {pack:?}")))?;
        // Drops `pack.toml`, keeping its folder.
        segments.pop();
        segments.pop_if_empty();
        segments.extend(relative.as_str().split('/'));
    }
    Ok(url.into())
}

fn unsafe_path(relative: &RelativePath) -> PackFileError {
    PackFileError::Packwiz(PackwizError::UnsafePath(relative.to_string()))
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> PackFileError {
    PackFileError::Io {
        path: path.display().to_string(),
        reason: error.to_string(),
    }
}

/// `relative` under `folder`, refusing anything that resolves (through
/// symbolic links) outside the canonical `folder`. Files that do not exist
/// yet resolve through their deepest existing ancestor; the missing tail
/// cannot hold links. Returns the canonical path.
fn resolve_inside(folder: &Path, relative: &RelativePath) -> Result<PathBuf, PackFileError> {
    let root = folder
        .canonicalize()
        .map_err(|error| io_error(folder, error))?;
    let mut existing = relative.under(&root);
    let mut missing = Vec::new();
    let resolved = loop {
        match existing.canonicalize() {
            Ok(canonical) => break canonical,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let Some(name) = existing.file_name().map(ToOwned::to_owned) else {
                    return Err(unsafe_path(relative));
                };
                missing.push(name);
                if !existing.pop() {
                    return Err(unsafe_path(relative));
                }
            }
            Err(error) => return Err(io_error(&existing, error)),
        }
    };
    if !resolved.starts_with(&root) {
        return Err(unsafe_path(relative));
    }
    Ok(missing
        .into_iter()
        .rev()
        .fold(resolved, |path, name| path.join(name)))
}

/// Reads a regular file, refusing it when it is larger than `limit`.
async fn read_capped(path: PathBuf, name: String, limit: usize) -> Result<Vec<u8>, PackFileError> {
    let task = tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path).map_err(|error| io_error(&path, error))?;
        let metadata = file.metadata().map_err(|error| io_error(&path, error))?;
        if !metadata.is_file() {
            return Err(io_error(&path, "not a regular file"));
        }
        let mut bytes = Vec::new();
        // Reading one byte past the cap catches files that grow after `metadata`.
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| io_error(&path, error))?;
        if bytes.len() > limit {
            return Err(PackFileError::TooLarge(name));
        }
        Ok(bytes)
    });
    task.await
        .map_err(|error| PackFileError::Source(format!("reading the pack failed: {error}")))?
}

/// The digest of `bytes` in `algorithm`: lowercase hex, or the decimal
/// CurseForge fingerprint for murmur2.
fn digest(bytes: &[u8], algorithm: HashAlgorithm) -> String {
    match algorithm {
        HashAlgorithm::Sha1 => hex::encode(sha1::Sha1::digest(bytes)),
        HashAlgorithm::Sha256 => hex::encode(sha2::Sha256::digest(bytes)),
        HashAlgorithm::Sha512 => hex::encode(sha2::Sha512::digest(bytes)),
        HashAlgorithm::Murmur2 => curseforge_fingerprint(bytes).to_string(),
    }
}

fn verify(name: &str, bytes: &[u8], expected: &Checksum) -> Result<(), PackFileError> {
    let actual = digest(bytes, expected.algorithm());
    if expected.matches(&actual) {
        Ok(())
    } else {
        Err(PackFileError::HashMismatch(format!(
            "{name} does not match its {:?} hash: expected {}, got {actual}",
            expected.algorithm(),
            expected.value()
        )))
    }
}

#[cfg(test)]
mod tests;
