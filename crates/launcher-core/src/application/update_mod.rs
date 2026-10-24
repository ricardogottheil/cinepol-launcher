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
