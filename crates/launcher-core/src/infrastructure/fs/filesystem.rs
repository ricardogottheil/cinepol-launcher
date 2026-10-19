use std::io::ErrorKind;
use std::path::Path;

use crate::domain::ports::{FileSystem, FileSystemError};

/// `FileSystem` backed by `tokio::fs`.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioFileSystem;

fn error(path: &Path, error: &std::io::Error) -> FileSystemError {
    FileSystemError {
        path: path.display().to_string(),
        reason: error.to_string(),
    }
}

impl FileSystem for TokioFileSystem {
    async fn exists(&self, path: &Path) -> bool {
        tokio::fs::try_exists(path).await.unwrap_or(false)
    }

    async fn create_dir_all(&self, path: &Path) -> Result<(), FileSystemError> {
        tokio::fs::create_dir_all(path)
            .await
            .map_err(|e| error(path, &e))
    }

    async fn remove_dir_all(&self, path: &Path) -> Result<(), FileSystemError> {
        match tokio::fs::remove_dir_all(path).await {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(error(path, &e)),
            _ => Ok(()),
        }
    }

    async fn newest_file_since(
        &self,
        dir: &Path,
        since: std::time::SystemTime,
    ) -> Result<Option<std::path::PathBuf>, FileSystemError> {
        let owned = dir.to_path_buf();
        tokio::task::spawn_blocking(move || newest(&owned, since))
            .await
            .map_err(|e| FileSystemError {
                path: String::new(),
                reason: e.to_string(),
            })?
    }

    async fn copy_dir_all(&self, from: &Path, to: &Path) -> Result<(), FileSystemError> {
        let (from, to) = (from.to_path_buf(), to.to_path_buf());
        tokio::task::spawn_blocking(move || copy_tree(&from, &to))
            .await
            .map_err(|e| FileSystemError {
                path: String::new(),
                reason: e.to_string(),
            })?
    }
}

fn newest(
    dir: &Path,
    since: std::time::SystemTime,
) -> Result<Option<std::path::PathBuf>, FileSystemError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(dir, &e)),
    };
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    // An entry that cannot be read (removed meanwhile, no permission) is
    // skipped: it must not hide a report that can be read.
    for entry in entries.flatten() {
        // `metadata` from the entry does not follow symlinks.
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified >= since && best.as_ref().is_none_or(|(time, _)| modified > *time) {
            best = Some((modified, entry.path()));
        }
    }
    Ok(best.map(|(_, path)| path))
}

/// `to` is created with `create_dir` (not `create_dir_all`) so an existing
/// directory fails instead of being merged into.
fn copy_tree(from: &Path, to: &Path) -> Result<(), FileSystemError> {
    if !std::fs::symlink_metadata(from)
        .map_err(|e| error(from, &e))?
        .is_dir()
    {
        return Err(error(from, &std::io::Error::other("not a directory")));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| error(parent, &e))?;
    }
    // Fails when `to` exists, so nothing below can touch a folder we did not create.
    std::fs::create_dir(to).map_err(|e| error(to, &e))?;
    let copied = copy_entries(from, to);
    if copied.is_err() {
        // Ours: created just above. Best effort; the copy error is what matters.
        let _ = std::fs::remove_dir_all(to);
    }
    copied
}

fn copy_entries(from: &Path, to: &Path) -> Result<(), FileSystemError> {
    let mut pending = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((source, target)) = pending.pop() {
        for entry in std::fs::read_dir(&source).map_err(|e| error(&source, &e))? {
            let entry = entry.map_err(|e| error(&source, &e))?;
            let kind = entry.file_type().map_err(|e| error(&entry.path(), &e))?;
            let destination = target.join(entry.file_name());
            if kind.is_dir() {
                std::fs::create_dir(&destination).map_err(|e| error(&destination, &e))?;
                pending.push((entry.path(), destination));
            } else if kind.is_file() {
                std::fs::copy(entry.path(), &destination).map_err(|e| error(&destination, &e))?;
            }
            // Symbolic links and special files are skipped.
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn creates_checks_and_removes_directories() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        let fs = TokioFileSystem;
        assert!(!fs.exists(&nested).await);
        fs.create_dir_all(&nested).await.unwrap();
        assert!(fs.exists(&nested).await);
        fs.remove_dir_all(&dir.path().join("a")).await.unwrap();
        assert!(!fs.exists(&nested).await);
        fs.remove_dir_all(&dir.path().join("missing"))
            .await
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_copy_removes_only_the_folder_it_created() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let locked = from.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("x"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let to = dir.path().join("to");

        let result = TokioFileSystem.copy_dir_all(&from, &to).await;

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert!(!to.exists());
    }

    #[tokio::test]
    async fn copies_a_tree_without_following_symlinks_or_merging() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        std::fs::create_dir_all(from.join("minecraft/saves/World")).unwrap();
        std::fs::write(from.join("instance.json"), b"{}").unwrap();
        std::fs::write(from.join("minecraft/saves/World/level.dat"), b"world").unwrap();
        std::fs::create_dir_all(from.join("empty")).unwrap();
        let outside = dir.path().join("secret.txt");
        std::fs::write(&outside, b"secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, from.join("link.txt")).unwrap();

        let fs = TokioFileSystem;
        let to = dir.path().join("to");
        fs.copy_dir_all(&from, &to).await.unwrap();

        assert_eq!(std::fs::read(to.join("instance.json")).unwrap(), b"{}");
        assert_eq!(
            std::fs::read(to.join("minecraft/saves/World/level.dat")).unwrap(),
            b"world"
        );
        assert!(to.join("empty").is_dir());
        assert!(std::fs::symlink_metadata(to.join("link.txt")).is_err());
        assert!(fs.copy_dir_all(&from, &to).await.is_err());
        // The refused copy left the existing folder as it was.
        assert_eq!(std::fs::read(to.join("instance.json")).unwrap(), b"{}");
        assert!(
            fs.copy_dir_all(&dir.path().join("missing"), &dir.path().join("x"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn finds_the_newest_file_written_since_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let fs = TokioFileSystem;
        let reports = dir.path().join("crash-reports");
        assert_eq!(
            fs.newest_file_since(&reports, std::time::SystemTime::UNIX_EPOCH)
                .await
                .unwrap(),
            None
        );
        std::fs::create_dir(&reports).unwrap();
        let before = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::write(reports.join("crash-a.txt"), b"a").unwrap();
        std::fs::create_dir(reports.join("folder")).unwrap();
        assert_eq!(
            fs.newest_file_since(&reports, before).await.unwrap(),
            Some(reports.join("crash-a.txt"))
        );
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        assert_eq!(fs.newest_file_since(&reports, later).await.unwrap(), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn entries_that_cannot_be_read_are_skipped_instead_of_failing_the_search() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let reports = dir.path().join("crash-reports");
        std::fs::create_dir(&reports).unwrap();
        std::fs::write(reports.join("crash-a.txt"), b"a").unwrap();
        // Without the execute bit the names can be listed but not stat'ed.
        std::fs::set_permissions(&reports, std::fs::Permissions::from_mode(0o644)).unwrap();

        let result = TokioFileSystem
            .newest_file_since(&reports, std::time::SystemTime::UNIX_EPOCH)
            .await;

        std::fs::set_permissions(&reports, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(result, Ok(None));
    }
}
