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
}
