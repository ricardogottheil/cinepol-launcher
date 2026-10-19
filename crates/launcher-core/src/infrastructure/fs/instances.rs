//! `instances/<id>/instance.json`: one file per instance.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::write_atomic;
use crate::domain::account::AccountId;
use crate::domain::instance::{
    EnvVars, GameVersion, Instance, InstanceId, InstanceName, InstanceOrigin, InstanceRepository,
    InstanceRepositoryError, JavaSelection, JvmArgs, LaunchSettings, MemorySettings, WindowSize,
};
use crate::domain::launch::LauncherPaths;
use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
use crate::domain::modpack::{ModpackRef, ModpackSourceKind};

const SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "instance.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstanceRecord {
    schema_version: u32,
    id: String,
    name: String,
    game_version: String,
    loader: LoaderRecord,
    origin: OriginRecord,
    #[serde(default)]
    settings: SettingsRecord,
    #[serde(default)]
    default_account: Option<Uuid>,
    #[serde(default)]
    playtime_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct LoaderRecord {
    kind: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum OriginRecord {
    Custom,
    CinepolServer,
    Modpack {
        source: String,
        project: String,
        version: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum JavaRecord {
    Managed,
    Custom { path: PathBuf },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsRecord {
    java: JavaRecord,
    memory_min_mb: u32,
    memory_max_mb: u32,
    #[serde(default)]
    jvm_args: Vec<String>,
    #[serde(default)]
    window: Option<(u32, u32)>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    update_pack_on_launch: bool,
}

impl Default for SettingsRecord {
    fn default() -> Self {
        let memory = MemorySettings::default();
        Self {
            java: JavaRecord::Managed,
            memory_min_mb: memory.min_mb(),
            memory_max_mb: memory.max_mb(),
            jvm_args: vec![],
            window: None,
            env: BTreeMap::new(),
            update_pack_on_launch: false,
        }
    }
}

fn source_id(kind: ModpackSourceKind) -> &'static str {
    match kind {
        ModpackSourceKind::Modrinth => "modrinth",
        ModpackSourceKind::CurseForge => "curseforge",
        ModpackSourceKind::Packwiz => "packwiz",
    }
}

impl From<&Instance> for InstanceRecord {
    fn from(instance: &Instance) -> Self {
        let settings = &instance.settings;
        Self {
            schema_version: SCHEMA_VERSION,
            id: instance.id.to_string(),
            name: instance.name.to_string(),
            game_version: instance.game_version.as_str().to_owned(),
            loader: LoaderRecord {
                kind: instance.loader.kind().id().to_owned(),
                version: instance.loader.version().map(|v| v.as_str().to_owned()),
            },
            origin: match &instance.origin {
                InstanceOrigin::Custom => OriginRecord::Custom,
                InstanceOrigin::CinepolServer => OriginRecord::CinepolServer,
                InstanceOrigin::Modpack(modpack) => OriginRecord::Modpack {
                    source: source_id(modpack.source).to_owned(),
                    project: modpack.project.clone(),
                    version: modpack.version.clone(),
                },
            },
            settings: SettingsRecord {
                java: match &settings.java {
                    JavaSelection::Managed => JavaRecord::Managed,
                    JavaSelection::Custom(path) => JavaRecord::Custom { path: path.clone() },
                },
                memory_min_mb: settings.memory.min_mb(),
                memory_max_mb: settings.memory.max_mb(),
                jvm_args: settings.jvm_args.as_slice().to_vec(),
                window: settings.window.map(|w| (w.width(), w.height())),
                env: settings
                    .env
                    .iter()
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect(),
                update_pack_on_launch: settings.update_pack_on_launch,
            },
            default_account: instance.default_account.map(|id| id.as_uuid()),
            playtime_secs: instance.playtime_secs,
        }
    }
}

impl TryFrom<InstanceRecord> for Instance {
    type Error = String;

    /// Every field goes back through the domain's validation, so a hand-edited
    /// file cannot smuggle an unsafe id, path or JVM argument into a launch.
    fn try_from(record: InstanceRecord) -> Result<Self, String> {
        let loader_kind = LoaderKind::from_id(&record.loader.kind).ok_or("unknown loader")?;
        let loader_version = record
            .loader
            .version
            .as_deref()
            .map(LoaderVersion::parse)
            .transpose()
            .map_err(|e| e.to_string())?;
        let origin = match record.origin {
            OriginRecord::Custom => InstanceOrigin::Custom,
            OriginRecord::CinepolServer => InstanceOrigin::CinepolServer,
            OriginRecord::Modpack {
                source,
                project,
                version,
            } => {
                let source = match source.as_str() {
                    "modrinth" => ModpackSourceKind::Modrinth,
                    "curseforge" => ModpackSourceKind::CurseForge,
                    "packwiz" => ModpackSourceKind::Packwiz,
                    _ => return Err(format!("unknown modpack source {source}")),
                };
                InstanceOrigin::Modpack(ModpackRef {
                    source,
                    project,
                    version,
                })
            }
        };
        let settings = record.settings;
        let window = settings
            .window
            .map(|(w, h)| WindowSize::new(w, h))
            .transpose()
            .map_err(|e| e.to_string())?;
        Ok(Instance {
            id: InstanceId::parse(&record.id).map_err(|e| e.to_string())?,
            name: InstanceName::parse(&record.name).map_err(|e| e.to_string())?,
            game_version: GameVersion::parse(&record.game_version).map_err(|e| e.to_string())?,
            loader: LoaderSpec::new(loader_kind, loader_version).map_err(|e| e.to_string())?,
            origin,
            settings: LaunchSettings {
                java: match settings.java {
                    JavaRecord::Managed => JavaSelection::Managed,
                    JavaRecord::Custom { path } => JavaSelection::Custom(path),
                },
                memory: MemorySettings::new(settings.memory_min_mb, settings.memory_max_mb)
                    .map_err(|e| e.to_string())?,
                jvm_args: JvmArgs::parse(settings.jvm_args).map_err(|e| e.to_string())?,
                window,
                env: EnvVars::parse(settings.env).map_err(|e| e.to_string())?,
                update_pack_on_launch: settings.update_pack_on_launch,
            },
            default_account: record.default_account.map(AccountId::from_uuid),
            playtime_secs: record.playtime_secs,
        })
    }
}

/// Instances stored under `LauncherPaths::instances_dir()`.
#[derive(Debug, Clone)]
pub struct JsonInstanceRepository {
    paths: LauncherPaths,
}

impl JsonInstanceRepository {
    pub fn new(paths: LauncherPaths) -> Self {
        Self { paths }
    }

    fn file(&self, id: &InstanceId) -> PathBuf {
        self.paths.instance_dir(id).join(FILE_NAME)
    }

    async fn read(&self, id: &InstanceId) -> Result<Instance, InstanceRepositoryError> {
        let bytes = match tokio::fs::read(self.file(id)).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return Err(InstanceRepositoryError::NotFound(id.clone()));
            }
            Err(e) => return Err(InstanceRepositoryError::Storage(e.to_string())),
        };
        let record: InstanceRecord = serde_json::from_slice(&bytes)
            .map_err(|e| InstanceRepositoryError::Storage(format!("{id}: {e}")))?;
        let instance = Instance::try_from(record)
            .map_err(|e| InstanceRepositoryError::Storage(format!("{id}: {e}")))?;
        if &instance.id != id {
            return Err(InstanceRepositoryError::Storage(format!(
                "{id}: file belongs to {}",
                instance.id
            )));
        }
        Ok(instance)
    }
}

impl InstanceRepository for JsonInstanceRepository {
    async fn list(&self) -> Result<Vec<Instance>, InstanceRepositoryError> {
        let mut entries = match tokio::fs::read_dir(self.paths.instances_dir()).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(InstanceRepositoryError::Storage(e.to_string())),
        };
        let mut instances = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| InstanceRepositoryError::Storage(e.to_string()))?
        {
            let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|name| InstanceId::parse(name).ok())
            else {
                continue;
            };
            match self.read(&id).await {
                Ok(instance) => instances.push(instance),
                Err(InstanceRepositoryError::NotFound(_)) => {}
                Err(error) => tracing::warn!(%error, "skipping an unreadable instance"),
            }
        }
        instances.sort_by(|a, b| {
            a.name
                .as_str()
                .to_lowercase()
                .cmp(&b.name.as_str().to_lowercase())
        });
        Ok(instances)
    }

    async fn get(&self, id: &InstanceId) -> Result<Instance, InstanceRepositoryError> {
        self.read(id).await
    }

    async fn save(&self, instance: &Instance) -> Result<(), InstanceRepositoryError> {
        let bytes = serde_json::to_vec_pretty(&InstanceRecord::from(instance))
            .map_err(|e| InstanceRepositoryError::Storage(e.to_string()))?;
        write_atomic(&self.file(&instance.id), &bytes)
            .await
            .map_err(|e| InstanceRepositoryError::Storage(e.to_string()))
    }

    async fn id_taken(&self, id: &InstanceId) -> Result<bool, InstanceRepositoryError> {
        // Any entry counts, readable or not, so a new instance never takes over a folder.
        tokio::fs::try_exists(self.paths.instance_dir(id))
            .await
            .map_err(|e| InstanceRepositoryError::Storage(e.to_string()))
    }

    async fn delete(&self, id: &InstanceId) -> Result<(), InstanceRepositoryError> {
        match tokio::fs::remove_dir_all(self.paths.instance_dir(id)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                Err(InstanceRepositoryError::NotFound(id.clone()))
            }
            Err(e) => Err(InstanceRepositoryError::Storage(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Instance {
        let mut instance = Instance::new(
            InstanceId::parse("survival-world").unwrap(),
            InstanceName::parse("Survival World").unwrap(),
            GameVersion::parse("26.2").unwrap(),
            LoaderSpec::new(
                LoaderKind::Fabric,
                Some(LoaderVersion::parse("0.17.2").unwrap()),
            )
            .unwrap(),
            InstanceOrigin::Modpack(ModpackRef {
                source: ModpackSourceKind::Modrinth,
                project: "1KVo5zza".into(),
                version: "abc".into(),
            }),
        );
        instance.settings.java = JavaSelection::Custom(PathBuf::from("/opt/jdk/bin/java"));
        instance.settings.jvm_args = JvmArgs::parse(vec!["-XX:+UseG1GC".into()]).unwrap();
        instance.settings.window = Some(WindowSize::new(1280, 720).unwrap());
        instance.default_account = Some(AccountId::new_random());
        instance.playtime_secs = 3600;
        instance
    }

    #[tokio::test]
    async fn round_trips_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonInstanceRepository::new(LauncherPaths::new(dir.path()));
        let instance = sample();
        repo.save(&instance).await.unwrap();
        assert_eq!(repo.get(&instance.id).await.unwrap(), instance);
        assert_eq!(repo.list().await.unwrap(), vec![instance]);
    }

    #[tokio::test]
    async fn reports_missing_instances_and_deletes_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonInstanceRepository::new(LauncherPaths::new(dir.path()));
        let id = InstanceId::parse("gone").unwrap();
        assert_eq!(
            repo.get(&id).await,
            Err(InstanceRepositoryError::NotFound(id.clone()))
        );
        assert_eq!(repo.list().await.unwrap(), vec![]);
        let instance = sample();
        repo.save(&instance).await.unwrap();
        repo.delete(&instance.id).await.unwrap();
        assert!(!dir.path().join("instances").join("survival-world").exists());
        assert_eq!(
            repo.delete(&instance.id).await,
            Err(InstanceRepositoryError::NotFound(instance.id.clone()))
        );
    }

    #[tokio::test]
    async fn unreadable_instance_folders_still_take_their_id() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonInstanceRepository::new(LauncherPaths::new(dir.path()));
        let broken = dir.path().join("instances").join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join(FILE_NAME), b"{not json").unwrap();

        assert!(repo.list().await.unwrap().is_empty());
        assert!(
            repo.id_taken(&InstanceId::parse("broken").unwrap())
                .await
                .unwrap()
        );
        assert!(
            !repo
                .id_taken(&InstanceId::parse("free").unwrap())
                .await
                .unwrap()
        );
        repo.save(&sample()).await.unwrap();
        assert!(repo.id_taken(&sample().id).await.unwrap());
    }

    #[tokio::test]
    async fn skips_corrupt_or_unsafe_files_when_listing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonInstanceRepository::new(LauncherPaths::new(dir.path()));
        repo.save(&sample()).await.unwrap();
        let broken = dir.path().join("instances").join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join(FILE_NAME), b"{not json").unwrap();
        let sneaky = dir.path().join("instances").join("sneaky");
        std::fs::create_dir_all(&sneaky).unwrap();
        let mut record = serde_json::to_value(InstanceRecord::from(&sample())).unwrap();
        record["id"] = "sneaky".into();
        record["settings"]["jvmArgs"] = serde_json::json!(["-Xmx64G"]);
        std::fs::write(sneaky.join(FILE_NAME), serde_json::to_vec(&record).unwrap()).unwrap();

        let listed = repo.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(matches!(
            repo.get(&InstanceId::parse("sneaky").unwrap()).await,
            Err(InstanceRepositoryError::Storage(_))
        ));
    }
}
