//! In-memory mod catalog and registry for the mod use cases' tests.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::domain::instance::InstanceId;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::modpack::{ModpackSourceError, PackFileError};
use crate::domain::mods::{
    DependencyKind, ModDependency, ModFile, ModPage, ModQuery, ModRegistry, ModRegistryStore,
    ModSource, ModSummary, ModTarget, ModVersion, ReleaseChannel,
};

use super::sha512_hex;

/// A mod catalog in memory. `versions` answers like the real catalog: only
/// the versions that fit the target, in the order they were added (add them
/// newest first). Records every project whose versions were asked for;
/// `projects_fail` makes the title lookup fail, `identify_fails` the hash
/// lookup. `identify` knows the jar of every version added.
#[derive(Debug, Default)]
pub struct FakeModSource {
    pub titles: HashMap<String, String>,
    pub versions: Vec<ModVersion>,
    pub asked: Mutex<Vec<String>>,
    pub projects_fail: bool,
    pub identify_fails: bool,
}

impl FakeModSource {
    /// Adds a project and its versions, newest first.
    pub fn with(mut self, project: &str, title: &str, versions: Vec<ModVersion>) -> Self {
        self.titles.insert(project.into(), title.into());
        self.versions.extend(versions);
        self
    }

    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl ModSource for FakeModSource {
    async fn search(&self, _query: &ModQuery) -> Result<ModPage, ModpackSourceError> {
        Ok(ModPage {
            items: Vec::new(),
            total: Some(0),
        })
    }

    async fn versions(
        &self,
        project: &str,
        target: &ModTarget,
    ) -> Result<Vec<ModVersion>, ModpackSourceError> {
        self.asked.lock().unwrap().push(project.into());
        Ok(self
            .versions
            .iter()
            .filter(|version| version.project == project && target.accepts(version))
            .cloned()
            .collect())
    }

    async fn version(&self, id: &str) -> Result<ModVersion, ModpackSourceError> {
        self.versions
            .iter()
            .find(|version| version.id == id)
            .cloned()
            .ok_or_else(|| ModpackSourceError::NotFound(id.into()))
    }

    async fn identify(
        &self,
        sha512: &[String],
    ) -> Result<HashMap<String, ModVersion>, ModpackSourceError> {
        if self.identify_fails {
            return Err(ModpackSourceError::Network("offline".into()));
        }
        Ok(self
            .versions
            .iter()
            .filter_map(|version| {
                let hash = version.file.as_ref()?.sha512.value().to_owned();
                sha512.contains(&hash).then(|| (hash, version.clone()))
            })
            .collect())
    }

    async fn projects(&self, ids: &[String]) -> Result<Vec<ModSummary>, ModpackSourceError> {
        if self.projects_fail {
            return Err(ModpackSourceError::RateLimited {
                retry_after_secs: 60,
            });
        }
        Ok(ids
            .iter()
            .filter_map(|id| {
                self.titles.get(id).map(|title| ModSummary {
                    project: id.clone(),
                    title: title.clone(),
                    description: String::new(),
                    icon_url: None,
                    downloads: None,
                })
            })
            .collect())
    }
}

/// Saves a Fabric 1.21.1 instance called `id` into `instances`.
pub async fn fabric_instance(
    instances: &crate::testing::InMemoryInstanceRepository,
    id: &InstanceId,
) {
    use crate::domain::instance::{
        GameVersion, Instance, InstanceName, InstanceOrigin, InstanceRepository,
    };
    use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
    let fabric = LoaderSpec::new(
        LoaderKind::Fabric,
        Some(LoaderVersion::parse("0.19.5").unwrap()),
    )
    .unwrap();
    instances
        .save(&Instance::new(
            id.clone(),
            InstanceName::parse("Survival").unwrap(),
            GameVersion::parse("1.21.1").unwrap(),
            fabric,
            InstanceOrigin::Custom,
        ))
        .await
        .unwrap();
}

/// The body a [`mod_version`]'s file downloads as, so hashes line up.
pub fn mod_jar(id: &str) -> Vec<u8> {
    format!("jar of {id}").into_bytes()
}

/// A release of `project` with a downloadable jar whose body is
/// [`mod_jar`]`(id)`, built for `game` on `loaders`.
pub fn mod_version(
    project: &str,
    id: &str,
    game: &str,
    loaders: &[crate::domain::loader::LoaderKind],
) -> ModVersion {
    use crate::domain::instance::GameVersion;
    let checksum = |algorithm, value: String| Checksum::parse(algorithm, &value).ok();
    ModVersion {
        id: id.into(),
        project: project.into(),
        name: id.into(),
        version_number: id.into(),
        channel: ReleaseChannel::Release,
        game_versions: GameVersion::parse(game).into_iter().collect(),
        loaders: loaders.to_vec(),
        published: None,
        file: checksum(HashAlgorithm::Sha512, sha512_hex(&mod_jar(id))).map(|sha512| ModFile {
            url: format!("https://cdn.modrinth.com/data/{project}/versions/{id}/{id}.jar"),
            filename: format!("{id}.jar"),
            size: mod_jar(id).len() as u64,
            sha512,
            sha1: None,
        }),
        dependencies: Vec::new(),
    }
}

/// `version` with one more dependency on `project` (and a pinned version).
pub fn depends(
    mut version: ModVersion,
    kind: DependencyKind,
    project: Option<&str>,
    pinned: Option<&str>,
) -> ModVersion {
    version.dependencies.push(ModDependency {
        project: project.map(Into::into),
        version: pinned.map(Into::into),
        kind,
    });
    version
}

/// Keeps registries in memory; `fail_saves` makes every save fail.
#[derive(Debug, Default)]
pub struct InMemoryModRegistryStore {
    pub registries: Mutex<HashMap<InstanceId, ModRegistry>>,
    fail_saves: AtomicBool,
}

impl InMemoryModRegistryStore {
    pub fn fail_saves(&self) {
        self.fail_saves.store(true, Ordering::SeqCst);
    }
}

impl ModRegistryStore for InMemoryModRegistryStore {
    async fn load(&self, id: &InstanceId) -> Result<ModRegistry, PackFileError> {
        Ok(self
            .registries
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default())
    }

    async fn save(&self, id: &InstanceId, registry: &ModRegistry) -> Result<(), PackFileError> {
        if self.fail_saves.load(Ordering::SeqCst) {
            return Err(PackFileError::Io {
                path: "mods.json".into(),
                reason: "disk full".into(),
            });
        }
        self.registries
            .lock()
            .unwrap()
            .insert(id.clone(), registry.clone());
        Ok(())
    }
}
