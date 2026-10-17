use std::path::{Path, PathBuf};

use super::extract::extract_zip_entries;
use crate::domain::launch::{ExtractError, NativeJar, NativesExtractor};

/// Extracts legacy natives jars into one directory: every file entry except
/// those under the jar's `exclude` prefixes (usually `META-INF/`). Directory
/// entries are skipped; unsafe entries fail the whole extraction.
#[derive(Debug, Clone, Copy, Default)]
pub struct ZipNativesExtractor;

impl NativesExtractor for ZipNativesExtractor {
    async fn extract(
        &self,
        jars: &[(PathBuf, NativeJar)],
        destination: &Path,
    ) -> Result<(), ExtractError> {
        let jars: Vec<(PathBuf, Vec<String>)> = jars
            .iter()
            .map(|(path, jar)| (path.clone(), jar.exclude.clone()))
            .collect();
        let destination = destination.to_path_buf();
        tokio::task::spawn_blocking(move || {
            for (jar, exclude) in &jars {
                extract_zip_entries(jar, &destination, exclude, false)?;
            }
            Ok(())
        })
        .await
        .map_err(|error| ExtractError::Io(error.to_string()))?
    }
}
