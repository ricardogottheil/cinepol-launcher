use std::io;
use std::path::Path;
use std::time::Duration;

const RENAME_ATTEMPTS: u32 = 5;

/// Writes `bytes` to a temporary file next to `path` and renames it over
/// `path`, so readers never see a half-written file. Renames are retried
/// because Windows refuses to replace a file another process holds open.
pub async fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    tokio::fs::create_dir_all(parent).await?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = parent.join(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    tokio::fs::write(&temp, bytes).await?;

    let mut attempt = 0;
    loop {
        match tokio::fs::rename(&temp, path).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt + 1 < RENAME_ATTEMPTS => {
                tracing::debug!(path = %path.display(), %error, "rename failed, retrying");
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => {
                let _ = tokio::fs::remove_file(&temp).await;
                return Err(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replaces_the_file_and_leaves_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("data.json");
        write_atomic(&path, b"first").await.unwrap();
        write_atomic(&path, b"second").await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"second");
        let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }
}
