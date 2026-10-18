//! [`PackFiles`] over `std::fs`, on the blocking thread pool.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};
use sha2::Sha512;

use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::RelativePath;
use crate::domain::modpack::sync::LocalFile;
use crate::domain::modpack::{PackFileError, PackFiles};

const BUFFER_BYTES: usize = 64 * 1024;

/// The game folder on the local disk.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdPackFiles;

impl PackFiles for StdPackFiles {
    async fn sha1(&self, path: &Path) -> Result<Option<String>, PackFileError> {
        let owned = path.to_path_buf();
        blocking(path, move || {
            let mut hasher = Sha1::new();
            match hash_file(&owned, |chunk| hasher.update(chunk)) {
                Ok(_) => Ok(Some(hex::encode(hasher.finalize()))),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
                Err(error) => Err(io_error(&owned, error)),
            }
        })
        .await
    }

    async fn place(&self, from: &Path, to: &Path) -> Result<(), PackFileError> {
        let (from, owned_to) = (from.to_path_buf(), to.to_path_buf());
        blocking(to, move || place(&from, &owned_to)).await
    }

    async fn remove_file(&self, path: &Path) -> Result<(), PackFileError> {
        let owned = path.to_path_buf();
        blocking(path, move || match fs::remove_file(&owned) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(io_error(&owned, error)),
            _ => Ok(()),
        })
        .await
    }

    async fn scan(&self, root: &Path, entries: &[&str]) -> Result<Vec<LocalFile>, PackFileError> {
        let owned = root.to_path_buf();
        let entries: Vec<String> = entries.iter().map(|entry| (*entry).to_owned()).collect();
        blocking(root, move || {
            let mut found = BTreeMap::new();
            for entry in entries {
                let relative = RelativePath::parse(&entry)
                    .map_err(|_| PackFileError::UnsafeEntry(entry.clone()))?;
                walk(&relative.under(&owned), relative.as_str(), &mut found)?;
            }
            Ok(found.into_values().collect())
        })
        .await
    }
}

async fn blocking<T: Send + 'static>(
    path: &Path,
    work: impl FnOnce() -> Result<T, PackFileError> + Send + 'static,
) -> Result<T, PackFileError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| io_error(path, error))?
}

fn io_error(path: &Path, reason: impl Display) -> PackFileError {
    PackFileError::Io {
        path: path.display().to_string(),
        reason: reason.to_string(),
    }
}

/// Streams a file through `update`; returns its size.
fn hash_file(path: &Path, mut update: impl FnMut(&[u8])) -> std::io::Result<u64> {
    let mut file = File::open(path)?;
    let mut buffer = vec![0; BUFFER_BYTES];
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(size);
        }
        size += read as u64;
        update(&buffer[..read]);
    }
}

/// Copies `from` into a temporary file next to `to`, then renames it over
/// `to` (replacing a file or a link, never writing through one).
fn place(from: &Path, to: &Path) -> Result<(), PackFileError> {
    let parent = to
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io_error(to, "the destination has no parent folder"))?;
    let name = to
        .file_name()
        .ok_or_else(|| io_error(to, "the destination has no file name"))?
        .to_string_lossy();
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let temp: PathBuf = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = fs::copy(from, &temp)
        .map_err(|error| io_error(from, error))
        .and_then(|_| fs::rename(&temp, to).map_err(|error| io_error(to, error)));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Adds every regular file at or under `path` (named `relative`) to `found`.
/// Links, other special files, missing paths and names that are not valid
/// UTF-8 relative paths are skipped.
fn walk(
    path: &Path,
    relative: &str,
    found: &mut BTreeMap<RelativePath, LocalFile>,
) -> Result<(), PackFileError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(path, error)),
    };
    if metadata.is_file() {
        let Ok(relative) = RelativePath::parse(relative) else {
            tracing::warn!(path = %path.display(), "skipping a file with an unsupported name");
            return Ok(());
        };
        let file = local_file(path, relative.clone())?;
        found.insert(relative, file);
    } else if metadata.is_dir() {
        for child in fs::read_dir(path).map_err(|error| io_error(path, error))? {
            let child = child.map_err(|error| io_error(path, error))?;
            let name = child.file_name();
            let Some(name) = name.to_str() else {
                tracing::warn!(path = %child.path().display(), "skipping a non UTF-8 file name");
                continue;
            };
            walk(&child.path(), &format!("{relative}/{name}"), found)?;
        }
    }
    Ok(())
}

fn local_file(path: &Path, relative: RelativePath) -> Result<LocalFile, PackFileError> {
    let (mut sha1, mut sha512) = (Sha1::new(), Sha512::new());
    let size = hash_file(path, |chunk| {
        sha1.update(chunk);
        sha512.update(chunk);
    })
    .map_err(|error| io_error(path, error))?;
    let checksum = |algorithm: HashAlgorithm, digest: &[u8]| {
        Checksum::parse(algorithm, &hex::encode(digest)).map_err(|error| io_error(path, error))
    };
    Ok(LocalFile {
        path: relative,
        sha1: checksum(HashAlgorithm::Sha1, &sha1.finalize())?,
        sha512: checksum(HashAlgorithm::Sha512, &sha512.finalize())?,
        size,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::testing::{sha1_hex, sha512_hex};

    fn write(root: &Path, relative: &str, body: &[u8]) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    #[tokio::test]
    async fn hashes_files_and_reports_missing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "a.txt", b"hello");
        let files = StdPackFiles;
        assert_eq!(files.sha1(&path).await.unwrap(), Some(sha1_hex(b"hello")));
        assert_eq!(files.sha1(&dir.path().join("missing")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn places_files_creating_folders_and_replacing_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let from = write(dir.path(), "staging/a.jar", b"new");
        let to = dir.path().join("game/mods/a.jar");
        let files = StdPackFiles;
        files.place(&from, &to).await.unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"new");

        let newer = write(dir.path(), "staging/b.jar", b"newer");
        files.place(&newer, &to).await.unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"newer");
        let names: Vec<_> = fs::read_dir(to.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a.jar"], "no temporary file is left behind");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn placing_over_a_link_replaces_the_link_not_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let outside = write(dir.path(), "outside.txt", b"keep");
        let to = dir.path().join("game/options.txt");
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &to).unwrap();
        let from = write(dir.path(), "staging/options.txt", b"pack");
        StdPackFiles.place(&from, &to).await.unwrap();
        assert_eq!(fs::read(&outside).unwrap(), b"keep");
        assert!(!fs::symlink_metadata(&to).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&to).unwrap(), b"pack");
    }

    #[tokio::test]
    async fn placing_a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = StdPackFiles
            .place(&dir.path().join("missing"), &dir.path().join("to"))
            .await;
        assert!(
            matches!(result, Err(PackFileError::Io { .. })),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn removes_files_and_ignores_missing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "a.txt", b"x");
        StdPackFiles.remove_file(&path).await.unwrap();
        assert!(!path.exists());
        StdPackFiles.remove_file(&path).await.unwrap();
    }

    #[tokio::test]
    async fn scans_regular_files_under_the_entries_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "mods/b.jar", b"b");
        write(root, "mods/a.jar", b"a");
        write(root, "config/deep/x.toml", b"x");
        write(root, "options.txt", b"fov:90");
        write(root, "saves/world/level.dat", b"world");
        fs::create_dir_all(root.join("resourcepacks")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("saves"), root.join("mods/linked")).unwrap();
            std::os::unix::fs::symlink(root.join("options.txt"), root.join("mods/l.jar")).unwrap();
        }
        let scanned = StdPackFiles
            .scan(
                root,
                &[
                    "mods",
                    "config",
                    "resourcepacks",
                    "shaderpacks",
                    "options.txt",
                ],
            )
            .await
            .unwrap();
        let paths: Vec<&str> = scanned.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "config/deep/x.toml",
                "mods/a.jar",
                "mods/b.jar",
                "options.txt"
            ]
        );
        let options = &scanned[3];
        assert_eq!(options.sha1.value(), sha1_hex(b"fov:90"));
        assert_eq!(options.sha512.value(), sha512_hex(b"fov:90"));
        assert_eq!(options.size, 6);
    }

    #[tokio::test]
    async fn scanning_refuses_entries_that_leave_the_root() {
        let dir = tempfile::tempdir().unwrap();
        for entry in ["../outside", "/etc", ""] {
            let result = StdPackFiles.scan(dir.path(), &[entry]).await;
            assert!(
                matches!(result, Err(PackFileError::UnsafeEntry(_))),
                "{entry}"
            );
        }
    }
}
