//! `.mrpack` archives: reading the index, extracting overrides, writing exports.
//!
//! Every entry name is validated before anything is written, with the same
//! zip-slip rules as [`super::extract_zip`], and every write target is
//! checked against the canonical staging folder. Size caps guard against zip
//! bombs: the index may hold [`MAX_INDEX_BYTES`], one override
//! [`DEFAULT_MAX_ENTRY_BYTES`] and all overrides together
//! [`DEFAULT_MAX_TOTAL_BYTES`] once uncompressed. They count the bytes actually
//! inflated, not the sizes the archive declares.

mod wire;

use std::collections::BTreeMap;
use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};
use zip::write::SimpleFileOptions;

use super::extract::{Root, entry_components};
use crate::domain::launch::{ExtractError, RelativePath};
use crate::domain::modpack::mrpack::{MRPACK_INDEX, MrpackIndex, RawMrpackIndex};
use crate::domain::modpack::{MrpackArchive, PackFileError, StagedFile};

/// Largest `modrinth.index.json` read (8 MiB; real packs are well under 1 MiB).
pub const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;
/// Largest single override file extracted (256 MiB).
pub const DEFAULT_MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
/// Largest total of override bytes extracted from one pack (1 GiB).
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

const OVERRIDES: &str = "overrides";
const CLIENT_OVERRIDES: &str = "client-overrides";

/// [`MrpackArchive`] over the `zip` crate, on the blocking thread pool.
#[derive(Debug, Clone, Copy)]
pub struct ZipMrpackArchive {
    max_entry_bytes: u64,
    max_total_bytes: u64,
}

impl Default for ZipMrpackArchive {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipMrpackArchive {
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_MAX_ENTRY_BYTES, DEFAULT_MAX_TOTAL_BYTES)
    }

    /// Other uncompressed size caps for overrides (per file, per pack).
    pub fn with_limits(max_entry_bytes: u64, max_total_bytes: u64) -> Self {
        Self {
            max_entry_bytes,
            max_total_bytes,
        }
    }
}

impl MrpackArchive for ZipMrpackArchive {
    async fn read_index(&self, archive: &Path) -> Result<MrpackIndex, PackFileError> {
        let path = archive.to_path_buf();
        blocking(archive, move || read_index(&path)).await
    }

    async fn extract_overrides(
        &self,
        archive: &Path,
        staging: &Path,
    ) -> Result<Vec<StagedFile>, PackFileError> {
        let (path, staging) = (archive.to_path_buf(), staging.to_path_buf());
        let limits = *self;
        blocking(archive, move || extract_overrides(&path, &staging, limits)).await
    }

    async fn write(
        &self,
        archive: &Path,
        index: &MrpackIndex,
        overrides: &[(RelativePath, PathBuf)],
    ) -> Result<(), PackFileError> {
        let (path, raw, overrides) = (archive.to_path_buf(), index.to_raw(), overrides.to_vec());
        blocking(archive, move || write(&path, raw, &overrides)).await
    }
}

async fn blocking<T: Send + 'static>(
    archive: &Path,
    work: impl FnOnce() -> Result<T, PackFileError> + Send + 'static,
) -> Result<T, PackFileError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| io_error(archive, error))?
}

fn io_error(path: &Path, reason: impl Display) -> PackFileError {
    PackFileError::Io {
        path: path.display().to_string(),
        reason: reason.to_string(),
    }
}

fn invalid(archive: &Path, reason: impl Display) -> PackFileError {
    PackFileError::InvalidArchive(format!("{}: {reason}", archive.display()))
}

fn from_extract(staging: &Path, error: ExtractError) -> PackFileError {
    match error {
        ExtractError::UnsafeEntry(name) => PackFileError::UnsafeEntry(name),
        ExtractError::Corrupt { path, reason } => {
            PackFileError::InvalidArchive(format!("{path}: {reason}"))
        }
        ExtractError::Io(reason) => io_error(staging, reason),
    }
}

fn open(archive: &Path) -> Result<zip::ZipArchive<File>, PackFileError> {
    let file = File::open(archive).map_err(|error| io_error(archive, error))?;
    zip::ZipArchive::new(file).map_err(|error| invalid(archive, error))
}

fn read_index(archive: &Path) -> Result<MrpackIndex, PackFileError> {
    let mut zip = open(archive)?;
    let entry = match zip.by_name(MRPACK_INDEX) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => {
            return Err(invalid(archive, format!("{MRPACK_INDEX} is missing")));
        }
        Err(error) => return Err(invalid(archive, error)),
    };
    let mut bytes = Vec::new();
    entry
        .take(MAX_INDEX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(archive, error))?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err(invalid(
            archive,
            format!("{MRPACK_INDEX} is larger than {MAX_INDEX_BYTES} bytes"),
        ));
    }
    let wire: wire::WireIndex =
        serde_json::from_slice(&bytes).map_err(|error| invalid(archive, error))?;
    Ok(MrpackIndex::validate(wire.into())?)
}

/// An override to extract: its index in the archive and where it goes.
struct Planned {
    index: usize,
    name: String,
    path: RelativePath,
}

/// Where an entry goes: `None` when it is not an override file (other
/// folders, `server-overrides/`, directories, links). An entry whose name is
/// unsafe anywhere in the archive is an error.
fn classify(name: &str, is_file: bool) -> Result<Option<(u8, RelativePath)>, PackFileError> {
    let unsafe_entry = || PackFileError::UnsafeEntry(name.to_owned());
    let components = entry_components(name).map_err(|_| unsafe_entry())?;
    let pass = match components.first() {
        Some(&OVERRIDES) => 0,
        Some(&CLIENT_OVERRIDES) => 1,
        _ => return Ok(None),
    };
    if !is_file || components.len() < 2 {
        return Ok(None);
    }
    // The raw name must be `<prefix>/<safe relative path>`: no backslashes,
    // empty segments or leading separators once the prefix is stripped.
    let prefix = if pass == 0 {
        OVERRIDES
    } else {
        CLIENT_OVERRIDES
    };
    let rest = name
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('/'))
        .ok_or_else(unsafe_entry)?;
    let path = RelativePath::parse(rest).map_err(|_| unsafe_entry())?;
    Ok(Some((pass, path)))
}

fn extract_overrides(
    archive: &Path,
    staging: &Path,
    limits: ZipMrpackArchive,
) -> Result<Vec<StagedFile>, PackFileError> {
    let mut zip = open(archive)?;

    // Check every name before writing anything.
    let mut planned: Vec<(u8, Planned)> = Vec::new();
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|error| invalid(archive, error))?;
        let name = entry
            .name()
            .map_err(|error| invalid(archive, error))?
            .into_owned();
        let is_file = !entry.is_dir() && !entry.is_symlink();
        if let Some((pass, path)) = classify(&name, is_file)? {
            planned.push((pass, Planned { index, name, path }));
        }
    }
    // `overrides/` first, then `client-overrides/`, each in archive order.
    planned.sort_by_key(|(pass, planned)| (*pass, planned.index));

    let root = Root::prepare(staging).map_err(|error| from_extract(staging, error))?;
    let mut staged = BTreeMap::new();
    let mut remaining = limits.max_total_bytes;
    for (_, planned) in planned {
        let mut entry = zip
            .by_index(planned.index)
            .map_err(|error| invalid(archive, error))?;
        let cap = limits.max_entry_bytes.min(remaining);
        let too_large = || {
            invalid(
                archive,
                format!("{} exceeds the override size cap", planned.name),
            )
        };
        if entry.size() > cap {
            return Err(too_large());
        }
        let components: Vec<&str> = planned.path.as_str().split('/').collect();
        let target = root.join(&components);
        root.prepare_parent(&target, &planned.name)
            .map_err(|error| from_extract(staging, error))?;
        Root::clear(&target).map_err(|error| from_extract(staging, error))?;
        let (written, sha1) = match copy_hashed(&mut entry, &target, cap) {
            Ok(Some(result)) => result,
            Ok(None) => {
                let _ = fs::remove_file(&target);
                return Err(too_large());
            }
            Err(error) => {
                let _ = fs::remove_file(&target);
                return Err(error);
            }
        };
        remaining -= written;
        staged.insert(
            planned.path.clone(),
            StagedFile {
                path: planned.path,
                sha1,
                staged: target,
            },
        );
    }
    Ok(staged.into_values().collect())
}

/// Copies at most `cap` bytes into a new file at `target`, hashing them.
/// `None` when the body is longer than `cap`.
fn copy_hashed(
    body: &mut impl Read,
    target: &Path,
    cap: u64,
) -> Result<Option<(u64, String)>, PackFileError> {
    let mut file = File::create(target).map_err(|error| io_error(target, error))?;
    let mut limited = body.take(cap + 1);
    let mut hasher = Sha1::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut written = 0_u64;
    loop {
        let read = limited
            .read(&mut buffer)
            .map_err(|error| io_error(target, error))?;
        if read == 0 {
            break;
        }
        written += read as u64;
        if written > cap {
            return Ok(None);
        }
        let chunk = &buffer[..read];
        hasher.update(chunk);
        file.write_all(chunk)
            .map_err(|error| io_error(target, error))?;
    }
    file.flush().map_err(|error| io_error(target, error))?;
    Ok(Some((written, hex::encode(hasher.finalize()))))
}

fn write(
    archive: &Path,
    index: RawMrpackIndex,
    overrides: &[(RelativePath, PathBuf)],
) -> Result<(), PackFileError> {
    let parent = archive
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io_error(archive, "the archive path has no parent folder"))?;
    let file_name = archive
        .file_name()
        .ok_or_else(|| io_error(archive, "the archive path has no file name"))?
        .to_string_lossy();
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let temp = parent.join(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let result = write_zip(&temp, index, overrides)
        .and_then(|()| fs::rename(&temp, archive).map_err(|error| io_error(archive, error)));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_zip(
    temp: &Path,
    index: RawMrpackIndex,
    overrides: &[(RelativePath, PathBuf)],
) -> Result<(), PackFileError> {
    let fail = |error: &dyn Display| io_error(temp, error);
    let json =
        serde_json::to_vec_pretty(&wire::WireIndex::from(index)).map_err(|error| fail(&error))?;
    let file = File::create(temp).map_err(|error| fail(&error))?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    zip.start_file(MRPACK_INDEX, options)
        .map_err(|error| fail(&error))?;
    zip.write_all(&json).map_err(|error| fail(&error))?;
    for (path, source) in overrides {
        let mut input = File::open(source).map_err(|error| io_error(source, error))?;
        zip.start_file(format!("{OVERRIDES}/{path}"), options)
            .map_err(|error| fail(&error))?;
        io::copy(&mut input, &mut zip).map_err(|error| io_error(source, error))?;
    }
    let writer = zip.finish().map_err(|error| fail(&error))?;
    let file = writer
        .into_inner()
        .map_err(|error| fail(&error.into_error()))?;
    file.sync_all().map_err(|error| fail(&error))
}

#[cfg(test)]
mod tests;
