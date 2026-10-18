use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::domain::launch::ExtractError;

/// Extracts a zip archive into `dest` (created when missing), skipping entries
/// whose name starts with one of `exclude_prefixes`. Returns the extracted files.
pub fn extract_zip(
    archive: &Path,
    dest: &Path,
    exclude_prefixes: &[String],
) -> Result<Vec<PathBuf>, ExtractError> {
    extract_zip_entries(archive, dest, exclude_prefixes, true)
}

/// Like [`extract_zip`]; `create_directories: false` skips directory entries
/// (parents of extracted files are still created).
pub(super) fn extract_zip_entries(
    archive: &Path,
    dest: &Path,
    exclude_prefixes: &[String],
    create_directories: bool,
) -> Result<Vec<PathBuf>, ExtractError> {
    let corrupt = |reason: String| ExtractError::Corrupt {
        path: archive.display().to_string(),
        reason,
    };
    let file = File::open(archive).map_err(|error| io_error(archive, &error))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| corrupt(error.to_string()))?;
    let root = Root::prepare(dest)?;

    let mut extracted = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| corrupt(error.to_string()))?;
        let name = entry
            .name()
            .map_err(|error| corrupt(error.to_string()))?
            .into_owned();
        let components = entry_components(&name)?;
        if components.is_empty()
            || exclude_prefixes
                .iter()
                .any(|prefix| name.starts_with(prefix.as_str()))
        {
            continue;
        }
        let path = root.join(&components);
        if entry.is_dir() {
            if create_directories {
                root.create_dir(&path, &name)?;
            }
        } else if entry.is_symlink() {
            let mut target = String::new();
            entry
                .read_to_string(&mut target)
                .map_err(|error| corrupt(error.to_string()))?;
            root.create_symlink(&path, &target, &name)?;
        } else {
            let mode = entry_mode(entry.unix_mode());
            root.write_file(&path, &mut entry, mode, &name)?;
            extracted.push(path);
        }
    }
    Ok(extracted)
}

/// Extracts a gzip-compressed tar archive into `dest` (created when missing).
/// Returns the extracted files (regular files and hard links).
pub fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<Vec<PathBuf>, ExtractError> {
    let corrupt = |reason: String| ExtractError::Corrupt {
        path: archive.display().to_string(),
        reason,
    };
    let file = File::open(archive).map_err(|error| io_error(archive, &error))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let root = Root::prepare(dest)?;

    let mut extracted = Vec::new();
    let entries = tar.entries().map_err(|error| corrupt(error.to_string()))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| corrupt(error.to_string()))?;
        let name = utf8_name(&entry.path_bytes())?;
        let components = entry_components(&name)?;
        if components.is_empty() {
            continue;
        }
        let path = root.join(&components);
        let kind = entry.header().entry_type();
        let link_name = || -> Result<String, ExtractError> {
            match entry.link_name_bytes() {
                Some(bytes) => utf8_name(&bytes),
                None => Err(ExtractError::UnsafeEntry(name.clone())),
            }
        };
        match kind {
            tar::EntryType::Directory => root.create_dir(&path, &name)?,
            tar::EntryType::Symlink => {
                let target = link_name()?;
                root.create_symlink(&path, &target, &name)?;
            }
            tar::EntryType::Link => {
                let target = link_name()?;
                root.create_hard_link(&path, &target, &name)?;
                extracted.push(path);
            }
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                let mode = entry.header().mode().ok();
                root.write_file(&path, &mut entry, entry_mode(mode), &name)?;
                extracted.push(path);
            }
            // Devices, FIFOs and extension headers are never needed by a runtime.
            _ => {}
        }
    }
    Ok(extracted)
}

/// Creates the symbolic link `relative` (an archive-style entry name) under
/// `root` pointing at `target`, with the same containment rules as archive
/// links: the link must resolve inside `root`. On Windows the target file is
/// copied instead.
pub(crate) fn create_link_inside(
    root: &Path,
    relative: &str,
    target: &str,
) -> Result<(), ExtractError> {
    let root = Root::prepare(root)?;
    let components = entry_components(relative)?;
    if components.is_empty() {
        return Err(ExtractError::UnsafeEntry(relative.to_owned()));
    }
    let path = root.join(&components);
    root.create_symlink(&path, target, relative)
}

/// [`extract_zip`] on the blocking thread pool.
pub async fn extract_zip_async(
    archive: PathBuf,
    dest: PathBuf,
    exclude_prefixes: Vec<String>,
) -> Result<Vec<PathBuf>, ExtractError> {
    tokio::task::spawn_blocking(move || extract_zip(&archive, &dest, &exclude_prefixes))
        .await
        .map_err(|error| ExtractError::Io(error.to_string()))?
}

/// [`extract_tar_gz`] on the blocking thread pool.
pub async fn extract_tar_gz_async(
    archive: PathBuf,
    dest: PathBuf,
) -> Result<Vec<PathBuf>, ExtractError> {
    tokio::task::spawn_blocking(move || extract_tar_gz(&archive, &dest))
        .await
        .map_err(|error| ExtractError::Io(error.to_string()))?
}

/// Splits an entry name into plain components. Rejects absolute paths, `..`,
/// drive letters, UNC prefixes and NUL bytes. `.` and empty segments are dropped.
pub(super) fn entry_components(name: &str) -> Result<Vec<&str>, ExtractError> {
    let unsafe_entry = || ExtractError::UnsafeEntry(name.to_owned());
    if name.starts_with(['/', '\\']) || name.contains([':', '\0']) {
        return Err(unsafe_entry());
    }
    let mut components = Vec::new();
    for segment in name.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => return Err(unsafe_entry()),
            plain => components.push(plain),
        }
    }
    Ok(components)
}

fn utf8_name(bytes: &[u8]) -> Result<String, ExtractError> {
    String::from_utf8(bytes.to_vec())
        .map_err(|_| ExtractError::UnsafeEntry(String::from_utf8_lossy(bytes).into_owned()))
}

/// Keeps only the executable bits that matter; `None` leaves the default mode.
fn entry_mode(mode: Option<u32>) -> Option<u32> {
    mode.filter(|mode| mode & 0o111 != 0)
        .map(|mode| (mode & 0o777) | 0o200)
}

fn io_error(path: &Path, error: &io::Error) -> ExtractError {
    ExtractError::Io(format!("{}: {error}", path.display()))
}

/// The canonicalized destination; every write is checked against it.
pub(super) struct Root {
    canonical: PathBuf,
}

impl Root {
    pub(super) fn prepare(dest: &Path) -> Result<Self, ExtractError> {
        fs::create_dir_all(dest).map_err(|error| io_error(dest, &error))?;
        let canonical = dest
            .canonicalize()
            .map_err(|error| io_error(dest, &error))?;
        Ok(Self { canonical })
    }

    pub(super) fn join(&self, components: &[&str]) -> PathBuf {
        components
            .iter()
            .fold(self.canonical.clone(), |path, part| path.join(part))
    }

    /// Canonicalizes `dir` and fails unless it is the root or below it.
    pub(super) fn check_inside(&self, dir: &Path, name: &str) -> Result<PathBuf, ExtractError> {
        let canonical = dir.canonicalize().map_err(|error| io_error(dir, &error))?;
        if canonical.starts_with(&self.canonical) {
            Ok(canonical)
        } else {
            Err(ExtractError::UnsafeEntry(name.to_owned()))
        }
    }

    /// Creates the parent of `path` and returns its canonical form, inside the root.
    pub(super) fn prepare_parent(&self, path: &Path, name: &str) -> Result<PathBuf, ExtractError> {
        let parent = path
            .parent()
            .ok_or_else(|| ExtractError::UnsafeEntry(name.to_owned()))?;
        // Check the deepest existing ancestor first so no directory is ever
        // created through a link that leads outside.
        let mut existing = parent;
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| ExtractError::UnsafeEntry(name.to_owned()))?;
        }
        self.check_inside(existing, name)?;
        fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
        self.check_inside(parent, name)
    }

    fn create_dir(&self, path: &Path, name: &str) -> Result<(), ExtractError> {
        self.prepare_parent(path, name)?;
        fs::create_dir_all(path).map_err(|error| io_error(path, &error))?;
        self.check_inside(path, name).map(|_| ())
    }

    /// Removes a file or link occupying `path` so nothing is written through it.
    pub(super) fn clear(path: &Path) -> Result<(), ExtractError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => Err(ExtractError::Io(format!(
                "{} is a directory",
                path.display()
            ))),
            Ok(_) => fs::remove_file(path).map_err(|error| io_error(path, &error)),
            Err(_) => Ok(()),
        }
    }

    fn write_file(
        &self,
        path: &Path,
        body: &mut impl Read,
        mode: Option<u32>,
        name: &str,
    ) -> Result<(), ExtractError> {
        self.prepare_parent(path, name)?;
        Self::clear(path)?;
        let mut file = File::create(path).map_err(|error| io_error(path, &error))?;
        io::copy(body, &mut file).map_err(|error| io_error(path, &error))?;
        set_mode(path, mode)
    }

    /// Only targets made of leading `..` followed by plain components are
    /// accepted, and the `..` part must stay inside the root from the link's
    /// real parent. Every link created this way resolves inside the root, so
    /// later links through it cannot escape either.
    fn create_symlink(&self, path: &Path, target: &str, name: &str) -> Result<(), ExtractError> {
        let unsafe_entry = || ExtractError::UnsafeEntry(name.to_owned());
        if target.is_empty()
            || target.starts_with(['/', '\\'])
            || target.contains([':', '\0', '\\'])
        {
            return Err(unsafe_entry());
        }
        let parent = self.prepare_parent(path, name)?;
        let mut resolved = parent;
        let mut descending = false;
        for segment in target.split('/') {
            match segment {
                "" | "." => {}
                ".." if descending => return Err(unsafe_entry()),
                ".." => {
                    if !resolved.pop() || !resolved.starts_with(&self.canonical) {
                        return Err(unsafe_entry());
                    }
                }
                plain => {
                    descending = true;
                    resolved.push(plain);
                }
            }
        }
        if !resolved.starts_with(&self.canonical) {
            return Err(unsafe_entry());
        }
        Self::clear(path)?;
        link_or_copy(target, &resolved, path)
    }

    /// Hard link targets are archive-root relative and must already exist inside.
    fn create_hard_link(&self, path: &Path, target: &str, name: &str) -> Result<(), ExtractError> {
        let components = entry_components(target)?;
        if components.is_empty() {
            return Err(ExtractError::UnsafeEntry(name.to_owned()));
        }
        let source = self.check_inside(&self.join(&components), name)?;
        if !source.is_file() {
            return Err(ExtractError::UnsafeEntry(name.to_owned()));
        }
        self.prepare_parent(path, name)?;
        Self::clear(path)?;
        fs::hard_link(&source, path)
            .or_else(|_| fs::copy(&source, path).map(|_| ()))
            .map_err(|error| io_error(path, &error))
    }
}

#[cfg(unix)]
fn link_or_copy(target: &str, _resolved: &Path, path: &Path) -> Result<(), ExtractError> {
    std::os::unix::fs::symlink(target, path).map_err(|error| io_error(path, &error))
}

/// Symlinks need privileges on Windows; copy the target file when it exists.
#[cfg(not(unix))]
fn link_or_copy(_target: &str, resolved: &Path, path: &Path) -> Result<(), ExtractError> {
    if resolved.is_file() {
        fs::copy(resolved, path).map_err(|error| io_error(path, &error))?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) -> Result<(), ExtractError> {
    use std::os::unix::fs::PermissionsExt;
    match mode {
        Some(mode) => fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| io_error(path, &error)),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) -> Result<(), ExtractError> {
    Ok(())
}
