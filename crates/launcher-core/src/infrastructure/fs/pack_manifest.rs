//! `instances/<id>/pack-manifest.json`: the files a modpack installed and
//! the files still waiting for a manual download.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::write_atomic;
use crate::domain::instance::InstanceId;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::{LauncherPaths, RelativePath};
use crate::domain::modpack::PackFileError;
use crate::domain::modpack::PackManifestStore;
use crate::domain::modpack::sync::{ManualFile, PackManifest};

const SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "pack-manifest.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestRecord {
    schema_version: u32,
    /// Path in the game folder to the lowercase hex sha1 the pack wrote.
    #[serde(default)]
    files: BTreeMap<String, String>,
    /// Files waiting for a manual download; absent in older manifests.
    #[serde(default)]
    pending: Vec<PendingRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingRecord {
    path: String,
    name: String,
    filename: String,
    /// `sha1`, `sha256`, `sha512` or `murmur2`.
    hash_format: String,
    hash: String,
    project_id: u64,
    file_id: u64,
}

fn algorithm_name(algorithm: HashAlgorithm) -> &'static str {
    match algorithm {
        HashAlgorithm::Sha1 => "sha1",
        HashAlgorithm::Sha256 => "sha256",
        HashAlgorithm::Sha512 => "sha512",
        HashAlgorithm::Murmur2 => "murmur2",
    }
}

impl PendingRecord {
    fn from_manual(file: &ManualFile) -> Self {
        Self {
            path: file.path.to_string(),
            name: file.name.clone(),
            filename: file.filename.clone(),
            hash_format: algorithm_name(file.checksum.algorithm()).to_owned(),
            hash: file.checksum.value().to_owned(),
            project_id: file.project_id,
            file_id: file.file_id,
        }
    }

    fn into_manual(self, manifest: &Path) -> Result<ManualFile, PackFileError> {
        let path = RelativePath::parse(&self.path)
            .map_err(|_| io_error(manifest, format!("unsafe pending path {:?}", self.path)))?;
        let single_segment =
            RelativePath::parse(&self.filename).is_ok() && !self.filename.contains('/');
        if !single_segment {
            return Err(io_error(
                manifest,
                format!("unsafe pending file name {:?}", self.filename),
            ));
        }
        let algorithm = HashAlgorithm::parse(&self.hash_format).ok_or_else(|| {
            io_error(
                manifest,
                format!(
                    "{}: unsupported hash format {:?}",
                    self.path, self.hash_format
                ),
            )
        })?;
        let checksum = Checksum::parse(algorithm, &self.hash)
            .map_err(|error| io_error(manifest, format!("{}: {error}", self.path)))?;
        Ok(ManualFile {
            path,
            name: self.name,
            filename: self.filename,
            checksum,
            project_id: self.project_id,
            file_id: self.file_id,
        })
    }
}

/// One JSON file per instance, written atomically.
#[derive(Debug, Clone)]
pub struct JsonPackManifestStore {
    paths: LauncherPaths,
}

impl JsonPackManifestStore {
    pub fn new(paths: LauncherPaths) -> Self {
        Self { paths }
    }

    fn file(&self, id: &InstanceId) -> PathBuf {
        self.paths.instance_dir(id).join(FILE_NAME)
    }
}

fn io_error(path: &Path, reason: impl Display) -> PackFileError {
    PackFileError::Io {
        path: path.display().to_string(),
        reason: reason.to_string(),
    }
}

fn parse(path: &Path, bytes: &[u8]) -> Result<PackManifest, PackFileError> {
    let record: ManifestRecord =
        serde_json::from_slice(bytes).map_err(|error| io_error(path, error))?;
    if record.schema_version != SCHEMA_VERSION {
        return Err(io_error(
            path,
            format!("unsupported schema version {}", record.schema_version),
        ));
    }
    let mut manifest = PackManifest::default();
    for (raw_path, sha1) in record.files {
        let relative = RelativePath::parse(&raw_path)
            .map_err(|_| io_error(path, format!("unsafe file path {raw_path:?}")))?;
        let sha1 = Checksum::parse(HashAlgorithm::Sha1, &sha1)
            .map_err(|error| io_error(path, format!("{raw_path}: {error}")))?;
        manifest.files.insert(relative, sha1.value().to_owned());
    }
    manifest.pending = record
        .pending
        .into_iter()
        .map(|pending| pending.into_manual(path))
        .collect::<Result<_, _>>()?;
    Ok(manifest)
}

impl PackManifestStore for JsonPackManifestStore {
    async fn load(&self, id: &InstanceId) -> Result<PackManifest, PackFileError> {
        let path = self.file(id);
        match tokio::fs::read(&path).await {
            Ok(bytes) => parse(&path, &bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(PackManifest::default()),
            Err(error) => Err(io_error(&path, error)),
        }
    }

    async fn save(&self, id: &InstanceId, manifest: &PackManifest) -> Result<(), PackFileError> {
        let path = self.file(id);
        let record = ManifestRecord {
            schema_version: SCHEMA_VERSION,
            files: manifest
                .files
                .iter()
                .map(|(relative, sha1)| (relative.to_string(), sha1.clone()))
                .collect(),
            pending: manifest
                .pending
                .iter()
                .map(PendingRecord::from_manual)
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&record).map_err(|error| io_error(&path, error))?;
        write_atomic(&path, &bytes)
            .await
            .map_err(|error| io_error(&path, error))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn store(dir: &Path) -> JsonPackManifestStore {
        JsonPackManifestStore::new(LauncherPaths::new(dir))
    }

    fn id() -> InstanceId {
        InstanceId::parse("fabulously-optimized").unwrap()
    }

    #[tokio::test]
    async fn a_missing_manifest_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            store(dir.path()).load(&id()).await.unwrap(),
            PackManifest::default()
        );
    }

    #[tokio::test]
    async fn round_trips_through_the_instance_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let manifest = PackManifest {
            files: [
                (RelativePath::parse("mods/a.jar").unwrap(), "a".repeat(40)),
                (RelativePath::parse("options.txt").unwrap(), "b".repeat(40)),
            ]
            .into(),
            pending: vec![jei()],
        };
        store.save(&id(), &manifest).await.unwrap();
        assert_eq!(store.load(&id()).await.unwrap(), manifest);
        let path = LauncherPaths::new(dir.path())
            .instance_dir(&id())
            .join("pack-manifest.json");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["files"]["mods/a.jar"], "a".repeat(40));
        assert_eq!(saved["pending"][0]["path"], "mods/jei.jar");
        assert_eq!(saved["pending"][0]["hashFormat"], "murmur2");
        assert_eq!(saved["pending"][0]["hash"], "3608199863");
        assert_eq!(saved["pending"][0]["projectId"], 238_222);
        assert_eq!(saved["pending"][0]["fileId"], 8_367_520);
    }

    fn jei() -> ManualFile {
        ManualFile {
            path: RelativePath::parse("mods/jei.jar").unwrap(),
            name: "Just Enough Items (JEI)".into(),
            filename: "jei-1.20.1-forge-15.20.0.133.jar".into(),
            checksum: Checksum::parse(HashAlgorithm::Murmur2, "3608199863").unwrap(),
            project_id: 238_222,
            file_id: 8_367_520,
        }
    }

    fn pending(path: &str, filename: &str, format: &str, hash: &str) -> String {
        format!(
            r#"{{"schemaVersion":1,"files":{{}},"pending":[{{"path":"{path}","name":"JEI","filename":"{filename}","hashFormat":"{format}","hash":"{hash}","projectId":1,"fileId":2}}]}}"#
        )
    }

    #[tokio::test]
    async fn manifests_without_pending_files_still_load() {
        let body = format!(
            r#"{{"schemaVersion":1,"files":{{"mods/a.jar":"{}"}}}}"#,
            "a".repeat(40)
        );
        let manifest = load_raw(&body).await.unwrap();
        assert!(manifest.pending.is_empty());
        assert_eq!(manifest.files.len(), 1);
    }

    #[tokio::test]
    async fn pending_files_are_validated_on_load() {
        let sha1 = "c".repeat(40);
        let valid = load_raw(&pending(
            "mods/jei.jar",
            "jei.jar",
            "SHA1",
            &sha1.to_uppercase(),
        ))
        .await
        .unwrap();
        assert_eq!(valid.pending[0].checksum.value(), sha1);
        assert_eq!(valid.pending[0].checksum.algorithm(), HashAlgorithm::Sha1);
        for body in [
            pending("../jei.jar", "jei.jar", "sha1", &sha1),
            pending("mods/jei.jar", "sub/jei.jar", "sha1", &sha1),
            pending("mods/jei.jar", "..", "sha1", &sha1),
            pending("mods/jei.jar", "jei.jar", "md5", &sha1),
            pending("mods/jei.jar", "jei.jar", "sha1", "short"),
            pending("mods/jei.jar", "jei.jar", "murmur2", "-1"),
            r#"{"schemaVersion":1,"pending":[{"path":"mods/a.jar"}]}"#.to_owned(),
        ] {
            let result = load_raw(&body).await;
            assert!(
                matches!(result, Err(PackFileError::Io { .. })),
                "{body}: {result:?}"
            );
        }
    }

    async fn load_raw(body: &str) -> Result<PackManifest, PackFileError> {
        let dir = tempfile::tempdir().unwrap();
        let path = LauncherPaths::new(dir.path())
            .instance_dir(&id())
            .join("pack-manifest.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        store(dir.path()).load(&id()).await
    }

    #[tokio::test]
    async fn invalid_manifests_are_errors() {
        let sha1 = "c".repeat(40);
        for body in [
            "not json".to_owned(),
            format!(r#"{{"schemaVersion":1,"files":{{"../evil":"{sha1}"}}}}"#),
            format!(r#"{{"schemaVersion":1,"files":{{"mods\\a.jar":"{sha1}"}}}}"#),
            r#"{"schemaVersion":1,"files":{"mods/a.jar":"short"}}"#.to_owned(),
            format!(r#"{{"schemaVersion":2,"files":{{"mods/a.jar":"{sha1}"}}}}"#),
        ] {
            let result = load_raw(&body).await;
            assert!(
                matches!(result, Err(PackFileError::Io { .. })),
                "{body}: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn digests_are_lowercased_and_unknown_fields_ignored() {
        let body = format!(
            r#"{{"schemaVersion":1,"files":{{"mods/a.jar":"{}"}},"extra":true}}"#,
            "D".repeat(40)
        );
        let manifest = load_raw(&body).await.unwrap();
        assert_eq!(
            manifest.files[&RelativePath::parse("mods/a.jar").unwrap()],
            "d".repeat(40)
        );
    }
}
