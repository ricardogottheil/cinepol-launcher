//! On-disk metadata cache: SHA-1 checks and atomic writes.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use sha1::{Digest, Sha1};

/// Lowercase hex SHA-1 of `bytes`.
pub(super) fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

/// The file's bytes, or `None` when it does not exist or cannot be read.
pub(super) async fn read_if_present(path: &Path) -> Option<Vec<u8>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring an unreadable cache file");
            None
        }
    }
}

/// Writes `bytes` to a temporary file next to `path`, then renames it into
/// place, so readers never see a partially written file.
pub(super) async fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cache path has no parent directory or file name",
        ));
    };
    tokio::fs::create_dir_all(dir).await?;
    let temp = dir.join(format!(
        ".{}.{}-{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = match tokio::fs::write(&temp, bytes).await {
        Ok(()) => tokio::fs::rename(&temp, path).await,
        Err(error) => Err(error),
    };
    if written.is_err() {
        let _ = tokio::fs::remove_file(&temp).await;
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_with_sha1() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[tokio::test]
    async fn writes_atomically_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versions").join("26.2").join("26.2.json");
        assert_eq!(read_if_present(&path).await, None);

        write_atomic(&path, b"first").await.unwrap();
        write_atomic(&path, b"second").await.unwrap();

        assert_eq!(read_if_present(&path).await, Some(b"second".to_vec()));
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary files are left behind");
    }
}
