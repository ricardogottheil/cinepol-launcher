//! `instances/<id>/mods.json`: the mods the launcher installed one by one,
//! and which mods need them.

use std::collections::BTreeSet;
use std::fmt::Display;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::write_atomic;
use crate::domain::instance::InstanceId;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::{LauncherPaths, RelativePath};
use crate::domain::modpack::PackFileError;
use crate::domain::mods::{InstalledMod, ModRegistry, ModRegistryStore};

const SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "mods.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryRecord {
    schema_version: u32,
    #[serde(default)]
    mods: Vec<ModRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModRecord {
    project: String,
    title: String,
    version: String,
    version_number: String,
    filename: String,
    sha512: String,
    requested: bool,
    #[serde(default)]
    required_by: BTreeSet<String>,
    #[serde(default)]
    incompatible: BTreeSet<String>,
}

impl ModRecord {
    fn from_installed(installed: &InstalledMod) -> Self {
        Self {
            project: installed.project.clone(),
            title: installed.title.clone(),
            version: installed.version.clone(),
            version_number: installed.version_number.clone(),
            filename: installed.filename.clone(),
            sha512: installed.sha512.clone(),
            requested: installed.requested,
            required_by: installed.required_by.clone(),
            incompatible: installed.incompatible.clone(),
        }
    }

    /// Refuses records whose file name could leave `mods/`, or whose hash is
    /// not a sha512: a hand-edited file must not point the launcher elsewhere.
    fn into_installed(self, registry: &Path) -> Result<InstalledMod, PackFileError> {
        if !RelativePath::is_file_name(&self.filename) {
            return Err(io_error(
                registry,
                format!("unsafe mod file name {:?}", self.filename),
            ));
        }
        let sha512 = Checksum::parse(HashAlgorithm::Sha512, &self.sha512)
            .map_err(|error| io_error(registry, format!("{}: {error}", self.filename)))?;
        Ok(InstalledMod {
            project: self.project,
            title: self.title,
            version: self.version,
            version_number: self.version_number,
            filename: self.filename,
            sha512: sha512.value().to_owned(),
            requested: self.requested,
            required_by: self.required_by,
            incompatible: self.incompatible,
        })
    }
}

/// One JSON file per instance, written atomically.
#[derive(Debug, Clone)]
pub struct JsonModRegistryStore {
    paths: LauncherPaths,
}

impl JsonModRegistryStore {
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

fn parse(path: &Path, bytes: &[u8]) -> Result<ModRegistry, PackFileError> {
    let record: RegistryRecord =
        serde_json::from_slice(bytes).map_err(|error| io_error(path, error))?;
    if record.schema_version != SCHEMA_VERSION {
        return Err(io_error(
            path,
            format!("unsupported schema version {}", record.schema_version),
        ));
    }
    let mods = record
        .mods
        .into_iter()
        .map(|installed| installed.into_installed(path))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ModRegistry::new(mods))
}

impl ModRegistryStore for JsonModRegistryStore {
    async fn load(&self, id: &InstanceId) -> Result<ModRegistry, PackFileError> {
        let path = self.file(id);
        match tokio::fs::read(&path).await {
            Ok(bytes) => parse(&path, &bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(ModRegistry::default()),
            Err(error) => Err(io_error(&path, error)),
        }
    }

    async fn save(&self, id: &InstanceId, registry: &ModRegistry) -> Result<(), PackFileError> {
        let path = self.file(id);
        let record = RegistryRecord {
            schema_version: SCHEMA_VERSION,
            mods: registry
                .mods()
                .iter()
                .map(ModRecord::from_installed)
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

    fn store(dir: &Path) -> JsonModRegistryStore {
        JsonModRegistryStore::new(LauncherPaths::new(dir))
    }

    fn id() -> InstanceId {
        InstanceId::parse("survival").unwrap()
    }

    fn iris() -> InstalledMod {
        InstalledMod {
            project: "YL57xq9U".into(),
            title: "Iris Shaders".into(),
            version: "zsoi0dso".into(),
            version_number: "1.8.8+1.21.1-fabric".into(),
            filename: "iris-fabric-1.8.8+mc1.21.1.jar".into(),
            sha512: "a".repeat(128),
            requested: true,
            required_by: BTreeSet::new(),
            incompatible: ["P7dR8mSH".to_owned()].into(),
        }
    }

    fn sodium() -> InstalledMod {
        InstalledMod {
            project: "AANobbMI".into(),
            title: "Sodium".into(),
            version: "s7adptIg".into(),
            version_number: "mc1.21.1-0.8.12-beta.1-fabric".into(),
            filename: "sodium-fabric-0.8.12-beta.1+mc1.21.1.jar".into(),
            sha512: "b".repeat(128),
            requested: false,
            required_by: ["YL57xq9U".to_owned()].into(),
            incompatible: BTreeSet::new(),
        }
    }

    #[tokio::test]
    async fn a_missing_registry_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            store(dir.path()).load(&id()).await.unwrap(),
            ModRegistry::default()
        );
    }

    #[tokio::test]
    async fn round_trips_through_the_instance_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let registry = ModRegistry::new([iris(), sodium()]);
        store.save(&id(), &registry).await.unwrap();
        assert_eq!(store.load(&id()).await.unwrap(), registry);

        let path = LauncherPaths::new(dir.path())
            .instance_dir(&id())
            .join("mods.json");
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["mods"][0]["project"], "YL57xq9U");
        assert_eq!(saved["mods"][0]["versionNumber"], "1.8.8+1.21.1-fabric");
        assert_eq!(saved["mods"][0]["incompatible"][0], "P7dR8mSH");
        assert_eq!(saved["mods"][1]["requested"], false);
        assert_eq!(saved["mods"][1]["requiredBy"][0], "YL57xq9U");
    }

    async fn load_raw(body: &str) -> Result<ModRegistry, PackFileError> {
        let dir = tempfile::tempdir().unwrap();
        let path = LauncherPaths::new(dir.path())
            .instance_dir(&id())
            .join("mods.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        store(dir.path()).load(&id()).await
    }

    fn record(filename: &str, sha512: &str) -> String {
        format!(
            r#"{{"schemaVersion":1,"mods":[{{"project":"p","title":"P","version":"v","versionNumber":"1","filename":"{filename}","sha512":"{sha512}","requested":true}}]}}"#
        )
    }

    #[tokio::test]
    async fn refuses_file_names_that_leave_the_mods_folder() {
        for filename in ["../evil.jar", "sub/evil.jar", "/abs.jar", ""] {
            assert!(
                load_raw(&record(filename, &"a".repeat(128))).await.is_err(),
                "{filename:?} was accepted"
            );
        }
        let loaded = load_raw(&record("ok.jar", &"A".repeat(128))).await.unwrap();
        // Hashes are kept lowercase.
        assert_eq!(loaded.mods()[0].sha512, "a".repeat(128));
    }

    #[tokio::test]
    async fn refuses_bad_hashes_and_unknown_schemas() {
        assert!(load_raw(&record("ok.jar", "short")).await.is_err());
        assert!(load_raw(r#"{"schemaVersion":2,"mods":[]}"#).await.is_err());
        assert!(load_raw("not json").await.is_err());
    }
}
