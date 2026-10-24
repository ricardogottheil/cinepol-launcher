//! Removes one mod jar from an instance's `mods/` folder, and optionally the
//! dependencies nothing else needs once it is gone.
//!
//! The jars are deleted first and the registry saved after; should saving
//! fail, the next read of `mods/` drops the records of the deleted jars.

use thiserror::Error;

use crate::application::install_mod::MODS_DIR;
use crate::application::mod_inventory::{checked_jar, read_mod_inventory};
use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::launch::LauncherPaths;
use crate::domain::modpack::{PackFileError, PackFiles};
use crate::domain::mods::{ModEntry, ModRegistry, ModRegistryStore, ModSource};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RemoveModError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Files(#[from] PackFileError),
    #[error("{0:?} is not the name of a mod jar")]
    NotAMod(String),
    #[error("mods/{0} was not found")]
    NotFound(String),
}

/// Everything removing a mod talks to.
pub struct RemoveModDeps<'a, I, S, P, M> {
    pub instances: &'a I,
    pub source: &'a S,
    pub files: &'a P,
    pub registry: &'a M,
    pub paths: &'a LauncherPaths,
}

/// Removes `filename` (its enabled name, `x.jar`, enabled or not) and, with
/// `remove_orphans`, the mods only it needed. Returns the names of the
/// files deleted, the requested one first.
pub async fn remove_mod<I, S, P, M>(
    deps: &RemoveModDeps<'_, I, S, P, M>,
    id: &InstanceId,
    filename: &str,
    remove_orphans: bool,
) -> Result<Vec<String>, RemoveModError>
where
    I: InstanceRepository,
    S: ModSource,
    P: PackFiles,
    M: ModRegistryStore,
{
    deps.instances.get(id).await?;
    checked_jar(filename).ok_or_else(|| RemoveModError::NotAMod(filename.to_owned()))?;
    let mods_dir = deps.paths.game_dir(id).join(MODS_DIR);
    let saved = deps.registry.load(id).await?;
    let inventory = read_mod_inventory(deps.source, deps.files, &mods_dir, &saved).await?;
    let mut registry = inventory.registry;
    let Some(entry) = inventory
        .entries
        .iter()
        .find(|entry| entry.filename == filename)
    else {
        return Err(RemoveModError::NotFound(filename.to_owned()));
    };

    let mut doomed = vec![entry];
    if remove_orphans {
        doomed.extend(
            entry
                .orphans
                .iter()
                .filter_map(|orphan| kept_copy(&inventory.entries, &registry, orphan)),
        );
    }
    let mut removed = Vec::with_capacity(doomed.len());
    for jar in &doomed {
        let name = jar.file_on_disk();
        deps.files.remove_file(&mods_dir.join(&name)).await?;
        removed.push(name);
    }
    let mut changed = false;
    for jar in &doomed {
        if let Some(record) = &jar.record
            && registry
                .get(&record.project)
                .is_some_and(|kept| kept.filename == jar.filename)
        {
            registry.remove(&record.project);
            changed = true;
        }
    }
    if changed || registry != saved {
        deps.registry.save(id, &registry).await?;
    }
    Ok(removed)
}

/// The jar the registry keeps for `project`.
fn kept_copy<'a>(
    entries: &'a [ModEntry],
    registry: &ModRegistry,
    project: &str,
) -> Option<&'a ModEntry> {
    let kept = registry.get(project)?;
    entries.iter().find(|entry| {
        entry.filename == kept.filename
            && entry
                .record
                .as_ref()
                .is_some_and(|record| record.project == project)
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;
    use crate::domain::mods::InstalledMod;
    use crate::testing::{
        FakeModSource, FakePackFiles, InMemoryInstanceRepository, InMemoryModRegistryStore,
        fabric_instance, sha512_hex,
    };

    struct World {
        _dir: tempfile::TempDir,
        paths: LauncherPaths,
        instances: InMemoryInstanceRepository,
        source: FakeModSource,
        files: FakePackFiles,
        registry: InMemoryModRegistryStore,
    }

    fn id() -> InstanceId {
        InstanceId::parse("survival").unwrap()
    }

    fn mods_dir(world: &World) -> PathBuf {
        world.paths.game_dir(&id()).join(MODS_DIR)
    }

    /// Iris needs Indium and Sodium; Indium needs Sodium; Lithium stands
    /// alone. Sodium is disabled.
    async fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let instances = InMemoryInstanceRepository::default();
        fabric_instance(&instances, &id()).await;
        let world = World {
            paths: LauncherPaths::new(dir.path()),
            _dir: dir,
            instances,
            source: FakeModSource::default(),
            files: FakePackFiles::default(),
            registry: InMemoryModRegistryStore::default(),
        };
        std::fs::create_dir_all(mods_dir(&world)).unwrap();
        let record = |project: &str, on_disk: &str, needers: &[&str]| {
            std::fs::write(mods_dir(&world).join(on_disk), project).unwrap();
            InstalledMod {
                project: project.into(),
                title: project.into(),
                version: format!("{project}-1"),
                version_number: "1".into(),
                filename: format!("{project}.jar"),
                sha512: sha512_hex(project.as_bytes()),
                requested: needers.is_empty(),
                required_by: needers.iter().map(|needer| (*needer).to_owned()).collect(),
                incompatible: BTreeSet::new(),
            }
        };
        let registry = ModRegistry::new([
            record("iris", "iris.jar", &[]),
            record("indium", "indium.jar", &["iris"]),
            record("sodium", "sodium.jar.disabled", &["iris", "indium"]),
            record("lithium", "lithium.jar", &[]),
        ]);
        world.registry.save(&id(), &registry).await.unwrap();
        world
    }

    async fn remove(
        world: &World,
        filename: &str,
        remove_orphans: bool,
    ) -> Result<Vec<String>, RemoveModError> {
        let deps = RemoveModDeps {
            instances: &world.instances,
            source: &world.source,
            files: &world.files,
            registry: &world.registry,
            paths: &world.paths,
        };
        remove_mod(&deps, &id(), filename, remove_orphans).await
    }

    fn jars_in(world: &World) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(mods_dir(world))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn removes_a_mod_with_the_dependencies_only_it_needed() {
        let world = world().await;
        let removed = remove(&world, "iris.jar", true).await.unwrap();
        assert_eq!(removed, ["iris.jar", "indium.jar", "sodium.jar.disabled"]);
        assert_eq!(jars_in(&world), ["lithium.jar"]);
        let registry = world.registry.load(&id()).await.unwrap();
        let left: Vec<&str> = registry.mods().iter().map(|m| m.project.as_str()).collect();
        assert_eq!(left, ["lithium"]);
    }

    #[tokio::test]
    async fn keeps_orphans_when_asked_and_records_that_nothing_needs_them() {
        let world = world().await;
        assert_eq!(
            remove(&world, "iris.jar", false).await.unwrap(),
            ["iris.jar"]
        );
        assert_eq!(
            jars_in(&world),
            ["indium.jar", "lithium.jar", "sodium.jar.disabled"]
        );
        let registry = world.registry.load(&id()).await.unwrap();
        assert!(registry.get("iris").is_none());
        assert!(registry.get("indium").unwrap().required_by.is_empty());
        assert_eq!(
            registry.get("sodium").unwrap().required_by,
            BTreeSet::from(["indium".to_owned()])
        );
    }

    #[tokio::test]
    async fn a_dependency_still_needed_elsewhere_is_never_an_orphan() {
        let world = world().await;
        let removed = remove(&world, "indium.jar", true).await.unwrap();
        assert_eq!(removed, ["indium.jar"]);
        assert!(jars_in(&world).contains(&"sodium.jar.disabled".to_owned()));
    }

    #[tokio::test]
    async fn removes_unknown_and_disabled_jars_by_their_enabled_name() {
        let world = world().await;
        std::fs::write(mods_dir(&world).join("custom.jar"), b"hand made").unwrap();
        assert_eq!(
            remove(&world, "custom.jar", true).await.unwrap(),
            ["custom.jar"]
        );
        assert_eq!(
            remove(&world, "sodium.jar", false).await.unwrap(),
            ["sodium.jar.disabled"]
        );
        let registry = world.registry.load(&id()).await.unwrap();
        assert!(registry.get("sodium").is_none());
    }

    #[tokio::test]
    async fn refuses_unsafe_names_and_missing_jars() {
        let world = world().await;
        assert_eq!(
            remove(&world, "../iris.jar", true).await,
            Err(RemoveModError::NotAMod("../iris.jar".into()))
        );
        assert_eq!(
            remove(&world, "ghost.jar", true).await,
            Err(RemoveModError::NotFound("ghost.jar".into()))
        );
        assert_eq!(jars_in(&world).len(), 4);
    }
}
