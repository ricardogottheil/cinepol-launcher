//! Updates one installed mod to its newest compatible version, with any
//! new required dependency, keeping why it is installed and whether it is
//! disabled. The old jar is put back when the update fails.

use tokio_util::sync::CancellationToken;

use crate::application::install_mod::{InstallModDeps, InstallModError, write_mod};
use crate::domain::instance::{InstanceId, InstanceRepository};
use crate::domain::modpack::PackFiles;
use crate::domain::mods::{InstalledMod, ModRegistryStore, ModSource};
use crate::domain::ports::{Downloader, FileSystem, ProgressReporter};

/// Updates `project` and returns the mods written, the updated one first.
pub async fn update_mod<I, S, D, F, P, M, R>(
    deps: &InstallModDeps<'_, I, S, D, F, P, M>,
    id: &InstanceId,
    project: &str,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<Vec<InstalledMod>, InstallModError>
where
    I: InstanceRepository,
    S: ModSource,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: ModRegistryStore,
    R: ProgressReporter,
{
    write_mod(deps, id, project, true, reporter, cancel).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;
    use crate::application::install_mod::{MODS_DIR, install_mod};
    use crate::domain::launch::LauncherPaths;
    use crate::domain::loader::LoaderKind::Fabric;
    use crate::domain::mods::DependencyKind::Required;
    use crate::testing::{
        FakeDownloader, FakeFileSystem, FakeModSource, FakePackFiles, InMemoryInstanceRepository,
        InMemoryModRegistryStore, RecordingReporter, depends, fabric_instance, mod_jar,
        mod_version,
    };

    struct World {
        _dir: tempfile::TempDir,
        paths: LauncherPaths,
        instances: InMemoryInstanceRepository,
        source: FakeModSource,
        fs: FakeFileSystem,
        files: FakePackFiles,
        registry: InMemoryModRegistryStore,
        downloader: FakeDownloader,
    }

    fn id() -> InstanceId {
        InstanceId::parse("survival").unwrap()
    }

    fn catalog(sodium: &[&str]) -> FakeModSource {
        FakeModSource::default()
            .with(
                "sodium",
                "Sodium",
                sodium
                    .iter()
                    .map(|version| mod_version("sodium", version, "1.21.1", &[Fabric]))
                    .collect(),
            )
            .with(
                "iris",
                "Iris",
                vec![depends(
                    mod_version("iris", "iris-2", "1.21.1", &[Fabric]),
                    Required,
                    Some("sodium"),
                    None,
                )],
            )
            .with(
                "fabric-api",
                "Fabric API",
                vec![mod_version(
                    "fabric-api",
                    "fabric-api-1",
                    "1.21.1",
                    &[Fabric],
                )],
            )
    }

    /// Sodium 2 installed for Iris, then the catalog moves on to Sodium 3.
    async fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let instances = InMemoryInstanceRepository::default();
        fabric_instance(&instances, &id()).await;
        let downloader = FakeDownloader::with(
            ["sodium-2", "sodium-3", "iris-2", "fabric-api-1"].map(|version| {
                let project = version.rsplit_once('-').unwrap().0;
                (
                    format!(
                        "https://cdn.modrinth.com/data/{project}/versions/{version}/{version}.jar"
                    ),
                    mod_jar(version),
                )
            }),
        );
        let mut world = World {
            paths: LauncherPaths::new(dir.path()),
            _dir: dir,
            instances,
            source: catalog(&["sodium-2"]),
            fs: FakeFileSystem::default(),
            files: FakePackFiles::default(),
            registry: InMemoryModRegistryStore::default(),
            downloader,
        };
        install_mod(
            &deps(&world),
            &id(),
            "iris",
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        world.source = catalog(&["sodium-3", "sodium-2"]);
        world
    }

    fn deps(
        world: &World,
    ) -> InstallModDeps<
        '_,
        InMemoryInstanceRepository,
        FakeModSource,
        FakeDownloader,
        FakeFileSystem,
        FakePackFiles,
        InMemoryModRegistryStore,
    > {
        InstallModDeps {
            instances: &world.instances,
            source: &world.source,
            downloader: &world.downloader,
            fs: &world.fs,
            files: &world.files,
            registry: &world.registry,
            paths: &world.paths,
        }
    }

    async fn update(world: &World, project: &str) -> Result<Vec<InstalledMod>, InstallModError> {
        update_mod(
            &deps(world),
            &id(),
            project,
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
    }

    fn mods_dir(world: &World) -> PathBuf {
        world.paths.game_dir(&id()).join(MODS_DIR)
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
    async fn replaces_the_jar_and_keeps_why_the_mod_is_installed() {
        let world = world().await;
        let written = update(&world, "sodium").await.unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(jars_in(&world), ["iris-2.jar", "sodium-3.jar"]);
        let registry = world.registry.load(&id()).await.unwrap();
        let sodium = registry.get("sodium").unwrap();
        assert_eq!(sodium.version, "sodium-3");
        assert_eq!(sodium.filename, "sodium-3.jar");
        assert!(!sodium.requested);
        assert_eq!(sodium.required_by, BTreeSet::from(["iris".to_owned()]));
    }

    #[tokio::test]
    async fn a_disabled_mod_stays_disabled() {
        let world = world().await;
        std::fs::rename(
            mods_dir(&world).join("sodium-2.jar"),
            mods_dir(&world).join("sodium-2.jar.disabled"),
        )
        .unwrap();
        update(&world, "sodium").await.unwrap();
        assert_eq!(jars_in(&world), ["iris-2.jar", "sodium-3.jar.disabled"]);
    }

    #[tokio::test]
    async fn brings_in_a_new_required_dependency() {
        let mut world = world().await;
        world.source = FakeModSource::default()
            .with(
                "sodium",
                "Sodium",
                vec![
                    depends(
                        mod_version("sodium", "sodium-3", "1.21.1", &[Fabric]),
                        Required,
                        Some("fabric-api"),
                        None,
                    ),
                    mod_version("sodium", "sodium-2", "1.21.1", &[Fabric]),
                ],
            )
            .with(
                "fabric-api",
                "Fabric API",
                vec![mod_version(
                    "fabric-api",
                    "fabric-api-1",
                    "1.21.1",
                    &[Fabric],
                )],
            );
        let written = update(&world, "sodium").await.unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(
            jars_in(&world),
            ["fabric-api-1.jar", "iris-2.jar", "sodium-3.jar"]
        );
        let registry = world.registry.load(&id()).await.unwrap();
        assert_eq!(
            registry.get("fabric-api").unwrap().required_by,
            BTreeSet::from(["sodium".to_owned()])
        );
    }

    #[tokio::test]
    async fn a_failed_update_puts_the_old_jar_back() {
        let world = world().await;
        world.registry.fail_saves();
        assert!(update(&world, "sodium").await.is_err());
        assert_eq!(jars_in(&world), ["iris-2.jar", "sodium-2.jar"]);
        assert_eq!(
            std::fs::read(mods_dir(&world).join("sodium-2.jar")).unwrap(),
            mod_jar("sodium-2")
        );
    }

    #[tokio::test]
    async fn a_dependency_the_new_version_drops_is_no_longer_needed_by_it() {
        let mut world = world().await;
        let with_api = |id: &str| {
            depends(
                mod_version("sodium", id, "1.21.1", &[Fabric]),
                Required,
                Some("fabric-api"),
                None,
            )
        };
        // Reinstall from a catalog where Sodium 2 needs Fabric API.
        world.source = catalog(&[]).with("sodium", "Sodium", vec![with_api("sodium-2")]);
        std::fs::remove_dir_all(mods_dir(&world)).unwrap();
        world
            .registry
            .save(&id(), &crate::domain::mods::ModRegistry::default())
            .await
            .unwrap();
        install_mod(
            &deps(&world),
            &id(),
            "iris",
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let registry = world.registry.load(&id()).await.unwrap();
        assert!(
            registry
                .get("fabric-api")
                .unwrap()
                .required_by
                .contains("sodium")
        );

        world.source = catalog(&["sodium-3", "sodium-2"]);
        update(&world, "sodium").await.unwrap();
        let registry = world.registry.load(&id()).await.unwrap();
        assert!(registry.get("fabric-api").unwrap().required_by.is_empty());
        // Nothing needs Fabric API any more.
        assert_eq!(
            registry.orphans_after_removing("iris"),
            ["fabric-api", "sodium"]
        );
    }

    #[tokio::test]
    async fn a_failed_placement_or_removal_leaves_the_old_jar_in_place() {
        let world = world().await;
        // The old jar is set aside, then writing the new one fails.
        world.files.fail_places_after(1);
        assert!(update(&world, "sodium").await.is_err());
        assert_eq!(jars_in(&world), ["iris-2.jar", "sodium-2.jar"]);

        world.files.fail_places_after(usize::MAX);
        world.files.fail_removes_after(0);
        assert!(update(&world, "sodium").await.is_err());
        assert!(jars_in(&world).contains(&"sodium-2.jar".to_owned()));
        let registry = world.registry.load(&id()).await.unwrap();
        assert_eq!(registry.get("sodium").unwrap().version, "sodium-2");
    }

    #[tokio::test]
    async fn a_new_jar_under_the_old_name_replaces_it_and_is_undone_on_failure() {
        let mut world = world().await;
        let same_name = |id: &str| {
            let mut version = mod_version("sodium", id, "1.21.1", &[Fabric]);
            if let Some(file) = version.file.as_mut() {
                file.filename = "sodium-2.jar".into();
            }
            version
        };
        world.source = catalog(&[]).with(
            "sodium",
            "Sodium",
            vec![same_name("sodium-3"), same_name("sodium-2")],
        );

        world.registry.fail_saves();
        assert!(update(&world, "sodium").await.is_err());
        assert_eq!(
            std::fs::read(mods_dir(&world).join("sodium-2.jar")).unwrap(),
            mod_jar("sodium-2")
        );
    }

    #[tokio::test]
    async fn the_old_jar_survives_in_scratch_when_it_cannot_be_put_back() {
        let world = world().await;
        // Setting the old jar aside and writing the new one work; putting
        // the old one back after the failed save does not.
        world.files.fail_places_after(2);
        world.registry.fail_saves();
        assert!(update(&world, "sodium").await.is_err());
        let mut kept = Vec::new();
        let mut stack = vec![world.paths.root().to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.file_name().is_some_and(|name| name == "previous.jar") {
                    kept.push(std::fs::read(path).unwrap());
                }
            }
        }
        assert_eq!(kept, [mod_jar("sodium-2")]);
    }

    #[tokio::test]
    async fn refuses_mods_that_are_current_or_not_installed() {
        let world = world().await;
        assert_eq!(
            update(&world, "iris").await,
            Err(InstallModError::UpToDate {
                title: "Iris".into()
            })
        );
        assert_eq!(
            update(&world, "fabric-api").await,
            Err(InstallModError::NotInstalled {
                project: "fabric-api".into()
            })
        );
        assert_eq!(jars_in(&world), ["iris-2.jar", "sodium-2.jar"]);
    }
}
