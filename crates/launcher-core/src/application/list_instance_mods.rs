//! Lists the mod jars in an instance's `mods/` folder: the ones the launcher
//! installed, the ones the catalog recognises by hash, and unknown ones.
//! Nothing is written.

use thiserror::Error;

use crate::application::install_mod::MODS_DIR;
use crate::application::mod_inventory::read_mod_inventory;
use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::launch::LauncherPaths;
use crate::domain::modpack::{PackFileError, PackFiles};
use crate::domain::mods::{ModEntry, ModRegistryStore, ModSource};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ListInstanceModsError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Files(#[from] PackFileError),
}

/// Everything listing an instance's mods talks to.
pub struct ListInstanceModsDeps<'a, I, S, P, M> {
    pub instances: &'a I,
    pub source: &'a S,
    pub files: &'a P,
    pub registry: &'a M,
    pub paths: &'a LauncherPaths,
}

/// Every mod jar in the instance's `mods/` folder, sorted by title.
pub async fn list_instance_mods<I, S, P, M>(
    deps: &ListInstanceModsDeps<'_, I, S, P, M>,
    id: &InstanceId,
) -> Result<Vec<ModEntry>, ListInstanceModsError>
where
    I: InstanceRepository,
    S: ModSource,
    P: PackFiles,
    M: ModRegistryStore,
{
    deps.instances.get(id).await?;
    let saved = deps.registry.load(id).await?;
    let mods_dir = deps.paths.game_dir(id).join(MODS_DIR);
    Ok(
        read_mod_inventory(deps.source, deps.files, &mods_dir, &saved)
            .await?
            .entries,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;
    use crate::domain::instance::{GameVersion, Instance, InstanceName, InstanceOrigin};
    use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
    use crate::domain::mods::{DependencyKind::*, InstalledMod, ModOrigin, ModRegistry};
    use crate::testing::{
        FakeModSource, FakePackFiles, InMemoryInstanceRepository, InMemoryModRegistryStore,
        depends, mod_jar, mod_version, sha512_hex,
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

    async fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(dir.path());
        let instances = InMemoryInstanceRepository::default();
        let fabric = LoaderSpec::new(
            LoaderKind::Fabric,
            Some(LoaderVersion::parse("0.19.5").unwrap()),
        )
        .unwrap();
        instances
            .save(&Instance::new(
                id(),
                InstanceName::parse("Survival").unwrap(),
                GameVersion::parse("1.21.1").unwrap(),
                fabric,
                InstanceOrigin::Custom,
            ))
            .await
            .unwrap();
        let source = FakeModSource::default()
            .with(
                "iris",
                "Iris Shaders",
                vec![depends(
                    mod_version("iris", "iris-2", "1.21.1", &[LoaderKind::Fabric]),
                    Incompatible,
                    Some("optifine"),
                    None,
                )],
            )
            .with(
                "sodium",
                "Sodium",
                vec![mod_version(
                    "sodium",
                    "sodium-2",
                    "1.21.1",
                    &[LoaderKind::Fabric],
                )],
            );
        World {
            _dir: dir,
            paths,
            instances,
            source,
            files: FakePackFiles::default(),
            registry: InMemoryModRegistryStore::default(),
        }
    }

    fn deps(
        world: &World,
    ) -> ListInstanceModsDeps<
        '_,
        InMemoryInstanceRepository,
        FakeModSource,
        FakePackFiles,
        InMemoryModRegistryStore,
    > {
        ListInstanceModsDeps {
            instances: &world.instances,
            source: &world.source,
            files: &world.files,
            registry: &world.registry,
            paths: &world.paths,
        }
    }

    fn mods_dir(world: &World) -> PathBuf {
        world.paths.game_dir(&id()).join(MODS_DIR)
    }

    fn put(world: &World, name: &str, bytes: &[u8]) {
        std::fs::create_dir_all(mods_dir(world)).unwrap();
        std::fs::write(mods_dir(world).join(name), bytes).unwrap();
    }

    fn recorded(project: &str, title: &str, filename: &str, bytes: &[u8]) -> InstalledMod {
        InstalledMod {
            project: project.into(),
            title: title.into(),
            version: format!("{project}-1"),
            version_number: "1".into(),
            filename: filename.into(),
            sha512: sha512_hex(bytes),
            requested: true,
            required_by: BTreeSet::new(),
            incompatible: BTreeSet::new(),
        }
    }

    #[tokio::test]
    async fn lists_launcher_identified_and_unknown_jars_sorted_by_title() {
        let world = world().await;
        put(&world, "lithium.jar", b"lithium bytes");
        put(&world, "sodium-2.jar.disabled", &mod_jar("sodium-2"));
        put(&world, "zzz-custom.jar", b"made by hand");
        put(&world, "readme.txt", b"not a mod");
        world
            .registry
            .save(
                &id(),
                &ModRegistry::new([recorded(
                    "lithium",
                    "Lithium",
                    "lithium.jar",
                    b"lithium bytes",
                )]),
            )
            .await
            .unwrap();

        let entries = list_instance_mods(&deps(&world), &id()).await.unwrap();
        let summary: Vec<(&str, &str, bool, ModOrigin)> = entries
            .iter()
            .map(|entry| {
                (
                    entry.title(),
                    entry.filename.as_str(),
                    entry.enabled,
                    entry.origin,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Lithium", "lithium.jar", true, ModOrigin::Launcher),
                ("Sodium", "sodium-2.jar", false, ModOrigin::Identified),
                ("zzz-custom.jar", "zzz-custom.jar", true, ModOrigin::Unknown),
            ]
        );
        let sodium = entries[1].record.as_ref().unwrap();
        assert_eq!(sodium.project, "sodium");
        assert_eq!(sodium.version, "sodium-2");
        assert!(sodium.requested);
        assert_eq!(entries[1].file_on_disk(), "sodium-2.jar.disabled");
        // Listing writes nothing.
        assert_eq!(world.registry.load(&id()).await.unwrap().mods().len(), 1);
    }

    #[tokio::test]
    async fn identified_jars_keep_their_declared_incompatibilities() {
        let world = world().await;
        put(&world, "iris-2.jar", &mod_jar("iris-2"));
        let entries = list_instance_mods(&deps(&world), &id()).await.unwrap();
        assert_eq!(
            entries[0].record.as_ref().unwrap().incompatible,
            BTreeSet::from(["optifine".to_owned()])
        );
    }

    #[tokio::test]
    async fn a_replaced_or_missing_jar_drops_its_record() {
        let world = world().await;
        // Recorded, but the file on disk is now another one.
        put(&world, "lithium.jar", b"another lithium");
        let mut sodium = recorded("sodium", "Sodium", "sodium.jar", b"gone");
        sodium.requested = false;
        sodium.required_by = BTreeSet::from(["lithium".to_owned()]);
        world
            .registry
            .save(
                &id(),
                &ModRegistry::new([
                    recorded("lithium", "Lithium", "lithium.jar", b"lithium bytes"),
                    sodium,
                ]),
            )
            .await
            .unwrap();
        let entries = list_instance_mods(&deps(&world), &id()).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin, ModOrigin::Unknown);
        assert_eq!(entries[0].title(), "lithium.jar");
    }

    #[tokio::test]
    async fn an_unreachable_catalog_lists_loose_jars_as_unknown() {
        let mut world = world().await;
        world.source.identify_fails = true;
        put(&world, "sodium-2.jar", &mod_jar("sodium-2"));
        let entries = list_instance_mods(&deps(&world), &id()).await.unwrap();
        assert_eq!(entries[0].origin, ModOrigin::Unknown);
    }

    #[tokio::test]
    async fn an_instance_without_mods_lists_nothing_and_unknown_ones_are_reported() {
        let world = world().await;
        assert!(
            list_instance_mods(&deps(&world), &id())
                .await
                .unwrap()
                .is_empty()
        );
        let other = InstanceId::parse("other").unwrap();
        assert!(matches!(
            list_instance_mods(&deps(&world), &other).await,
            Err(ListInstanceModsError::Instance(_))
        ));
    }
}
