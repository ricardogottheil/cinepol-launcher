//! CurseForge pack zips: the size-capped `manifest.json` and the overrides
//! folder it names, extracted with the same zip-slip rules and size caps as
//! `.mrpack` overrides.

use std::io::Read;
use std::path::Path;

use super::mrpack::{MAX_INDEX_BYTES, ZipMrpackArchive, blocking, extract_folders, invalid, open};
use crate::domain::modpack::curseforge::RawManifest;
use crate::domain::modpack::{CurseForgeArchive, PackFileError, StagedFile};
use crate::infrastructure::curseforge::parse_manifest;

const MANIFEST: &str = "manifest.json";

/// [`CurseForgeArchive`] over the `zip` crate, on the blocking thread pool.
#[derive(Debug, Clone, Copy, Default)]
pub struct ZipCurseForgeArchive {
    limits: ZipMrpackArchive,
}

impl ZipCurseForgeArchive {
    /// Other uncompressed size caps for overrides (per file, per pack).
    pub fn with_limits(max_entry_bytes: u64, max_total_bytes: u64) -> Self {
        Self {
            limits: ZipMrpackArchive::with_limits(max_entry_bytes, max_total_bytes),
        }
    }
}

impl CurseForgeArchive for ZipCurseForgeArchive {
    async fn read_manifest(&self, archive: &Path) -> Result<RawManifest, PackFileError> {
        let path = archive.to_path_buf();
        blocking(archive, move || read_manifest(&path)).await
    }

    async fn extract_overrides(
        &self,
        archive: &Path,
        folder: &str,
        staging: &Path,
    ) -> Result<Vec<StagedFile>, PackFileError> {
        let (path, staging, folder) = (
            archive.to_path_buf(),
            staging.to_path_buf(),
            folder.to_owned(),
        );
        let limits = self.limits;
        blocking(archive, move || {
            extract_folders(&path, &staging, limits, &[folder.as_str()])
        })
        .await
    }
}

fn read_manifest(archive: &Path) -> Result<RawManifest, PackFileError> {
    let mut zip = open(archive)?;
    let entry = match zip.by_name(MANIFEST) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => {
            return Err(invalid(archive, format!("{MANIFEST} is missing")));
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
            format!("{MANIFEST} is larger than {MAX_INDEX_BYTES} bytes"),
        ));
    }
    parse_manifest(&bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use zip::write::SimpleFileOptions;

    use super::*;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/curseforge/fabulously-optimized-14.1.1.zip")
    }

    fn zip_with(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.join("pack.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, body) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    const MANIFEST_JSON: &[u8] = br#"{"minecraft":{"version":"26.2","modLoaders":[]},"manifestType":"minecraftModpack","manifestVersion":1,"name":"P","files":[],"overrides":"extra"}"#;

    #[tokio::test]
    async fn reads_the_manifest_of_the_real_pack() {
        let manifest = ZipCurseForgeArchive::default()
            .read_manifest(&fixture())
            .await
            .unwrap();
        assert_eq!(manifest.name, "Fabulously Optimized");
        assert_eq!(manifest.version.as_deref(), Some("14.1.1"));
        assert_eq!(manifest.game_version, "26.2");
        assert_eq!(manifest.files.len(), 51);
        assert_eq!(manifest.overrides.as_deref(), Some("overrides"));
    }

    #[tokio::test]
    async fn extracts_the_real_overrides_without_the_folder_prefix() {
        let staging = tempfile::tempdir().unwrap();
        let staged = ZipCurseForgeArchive::default()
            .extract_overrides(&fixture(), "overrides", staging.path())
            .await
            .unwrap();
        assert_eq!(staged.len(), 57);
        assert!(
            staged
                .iter()
                .all(|file| !file.path.as_str().starts_with("overrides/"))
        );
        let loader = staged
            .iter()
            .find(|file| file.path.as_str() == "config/fabric_loader_dependencies.json")
            .unwrap();
        assert!(loader.staged.starts_with(staging.path().canonicalize().unwrap()));
        assert_eq!(loader.sha1.len(), 40);
        assert!(loader.staged.is_file());
    }

    #[tokio::test]
    async fn uses_the_folder_the_manifest_names_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let zip = zip_with(
            dir.path(),
            &[
                ("manifest.json", MANIFEST_JSON),
                ("extra/config/a.txt", b"a"),
                ("overrides/config/b.txt", b"b"),
                ("modlist.html", b"<html>"),
            ],
        );
        let archive = ZipCurseForgeArchive::default();
        assert_eq!(
            archive
                .read_manifest(&zip)
                .await
                .unwrap()
                .overrides
                .as_deref(),
            Some("extra")
        );
        let staging = dir.path().join("staging");
        let staged = archive
            .extract_overrides(&zip, "extra", &staging)
            .await
            .unwrap();
        let paths: Vec<&str> = staged.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["config/a.txt"]);
    }

    #[tokio::test]
    async fn refuses_unsafe_entries_a_missing_manifest_and_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let slip = zip_with(dir.path(), &[("overrides/../../evil.txt", b"x")]);
        let staging = dir.path().join("staging");
        assert!(matches!(
            ZipCurseForgeArchive::default()
                .extract_overrides(&slip, "overrides", &staging)
                .await,
            Err(PackFileError::UnsafeEntry(_))
        ));
        assert!(!dir.path().join("evil.txt").exists());

        let empty = zip_with(dir.path(), &[("overrides/a.txt", b"a")]);
        assert!(matches!(
            ZipCurseForgeArchive::default().read_manifest(&empty).await,
            Err(PackFileError::InvalidArchive(_))
        ));

        let big = zip_with(dir.path(), &[("overrides/big.bin", &[0u8; 64])]);
        assert!(matches!(
            ZipCurseForgeArchive::with_limits(16, 1024)
                .extract_overrides(&big, "overrides", &staging)
                .await,
            Err(PackFileError::InvalidArchive(_))
        ));
    }
}
