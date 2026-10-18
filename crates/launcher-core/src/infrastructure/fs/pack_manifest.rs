//! `instances/<id>/pack-manifest.json`: the files a modpack installed.

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
use crate::domain::modpack::sync::PackManifest;

const SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "pack-manifest.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestRecord {
    schema_version: u32,
    /// Path in the game folder to the lowercase hex sha1 the pack wrote.
    #[serde(default)]
    files: BTreeMap<String, String>,
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
