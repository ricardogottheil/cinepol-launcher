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
    std::fs::create_dir(to).map_err(|e| error(to, &e))?;
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
        assert!(
            fs.copy_dir_all(&dir.path().join("missing"), &dir.path().join("x"))
                .await
                .is_err()
        );
    }
}
