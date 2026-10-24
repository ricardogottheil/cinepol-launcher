//! Installs one mod into an instance with the required dependencies it
//! pulls in, and records why each one is there.
//!
//! Every jar is downloaded into a scratch folder and verified against its
//! sha512 there; only once all of them arrived are they moved into `mods/`,
//! so a failed or cancelled install leaves the instance untouched.

use std::path::Path;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::modpack_files::{release_dir, scratch_dir};
use crate::application::plan_mod_install::plan_mod_install;
use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::launch::LauncherPaths;
use crate::domain::modpack::{PackFileError, PackFiles};
use crate::domain::mods::{
    InstalledMod, ModFile, ModPlanError, ModRegistryStore, ModSource, ModTarget, PlannedMod,
};
use crate::domain::ports::{
    DownloadError, DownloadRequest, Downloader, FileSystem, FileSystemError, ProgressReporter,
    ProgressStage, StagedReporter,
};

/// The folder of the game directory mods are loaded from.
pub const MODS_DIR: &str = "mods";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstallModError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Plan(#[from] ModPlanError),
    #[error(transparent)]
    Download(DownloadError),
    #[error(transparent)]
    Files(#[from] PackFileError),
    #[error(transparent)]
    Scratch(#[from] FileSystemError),
    #[error("the mod installation was cancelled")]
    Cancelled,
}

impl From<DownloadError> for InstallModError {
    fn from(error: DownloadError) -> Self {
        if error == DownloadError::Cancelled {
            Self::Cancelled
        } else {
            Self::Download(error)
        }
    }
}

/// Everything the mod installer talks to.
pub struct InstallModDeps<'a, I, S, D, F, P, M> {
    pub instances: &'a I,
    pub source: &'a S,
    pub downloader: &'a D,
    pub fs: &'a F,
    pub files: &'a P,
    pub registry: &'a M,
    pub paths: &'a LauncherPaths,
}

/// Installs `project` into the instance and returns the mods it wrote, the
/// requested one first.
pub async fn install_mod<I, S, D, F, P, M, R>(
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
    let instance = deps.instances.get(id).await?;
    let target = ModTarget {
        game_version: instance.game_version.clone(),
        loader: instance.loader.kind(),
    };
    let mut registry = deps.registry.load(id).await?;
    let plan = plan_mod_install(deps.source, &target, &registry, project).await?;
    if cancel.is_cancelled() {
        return Err(InstallModError::Cancelled);
    }

    let staging = scratch_dir(deps.paths, "mods");
    let written = write_jars(deps, id, &plan.mods, &staging, reporter, cancel).await;
    release_dir(deps.fs, &staging).await;
    let written = written?;

    for (dependency, needer) in &plan.present {
        if let Some(existing) = registry.get_mut(dependency) {
            existing.required_by.insert(needer.clone());
        }
    }
    for installed in &written {
        registry.insert(installed.clone());
    }
    deps.registry.save(id, &registry).await?;
    Ok(written)
}

/// Downloads every planned jar into `staging`, verified, then moves them all
/// into the instance's `mods/` folder.
async fn write_jars<I, S, D, F, P, M, R>(
    deps: &InstallModDeps<'_, I, S, D, F, P, M>,
    id: &InstanceId,
    planned: &[PlannedMod],
    staging: &Path,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<Vec<InstalledMod>, InstallModError>
where
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    R: ProgressReporter,
{
    deps.fs.create_dir_all(staging).await?;
    // Planned versions always have a file: the plan only picks installable ones.
    let jars: Vec<(&PlannedMod, &ModFile)> = planned
        .iter()
        .filter_map(|planned| planned.version.file.as_ref().map(|file| (planned, file)))
        .collect();
    let requests = jars
        .iter()
        .enumerate()
        .map(|(index, (_, file))| DownloadRequest {
            url: file.url.clone(),
            // The catalog's file name is not trusted as a scratch path.
            destination: staging.join(format!("{index}.jar")),
            checksum: Some(file.sha512.clone()),
            size: Some(file.size),
        })
        .collect();
    deps.downloader
        .download_all(
            requests,
            &StagedReporter::new(reporter, ProgressStage::Mods),
            cancel,
        )
        .await?;
    if cancel.is_cancelled() {
        return Err(InstallModError::Cancelled);
    }

    let mods_dir = deps.paths.game_dir(id).join(MODS_DIR);
    let mut written = Vec::with_capacity(jars.len());
    for (index, (planned, file)) in jars.into_iter().enumerate() {
        // A single safe file name: checked when the catalog answer was read.
        deps.files
            .place(
                &staging.join(format!("{index}.jar")),
                &mods_dir.join(&file.filename),
            )
            .await?;
        let version = &planned.version;
        written.push(InstalledMod {
            project: version.project.clone(),
            title: planned.title.clone(),
            version: version.id.clone(),
            version_number: version.version_number.clone(),
            filename: file.filename.clone(),
            sha512: file.sha512.value().to_owned(),
            requested: planned.required_by.is_none(),
            required_by: planned.required_by.iter().cloned().collect(),
            incompatible: version
                .dependencies
                .iter()
                .filter(|dependency| {
                    dependency.kind == crate::domain::mods::DependencyKind::Incompatible
                })
                .filter_map(|dependency| dependency.project.clone())
                .collect(),
        });
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::instance::{GameVersion, Instance, InstanceName, InstanceOrigin};
    use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
    use crate::domain::mods::{DependencyKind::*, ModRegistry};
    use crate::testing::{
        FakeDownloader, FakeFileSystem, FakeModSource, FakePackFiles, InMemoryInstanceRepository,
        InMemoryModRegistryStore, RecordingReporter, depends, mod_jar, mod_version,
    };

    struct World {
        dir: tempfile::TempDir,
        paths: LauncherPaths,
        instances: InMemoryInstanceRepository,
        source: FakeModSource,
        fs: FakeFileSystem,
        files: FakePackFiles,
        registry: InMemoryModRegistryStore,
    }

    fn id() -> InstanceId {
        InstanceId::parse("survival").unwrap()
    }

    async fn world(loader: LoaderKind) -> World {
        let dir = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(dir.path());
        let instances = InMemoryInstanceRepository::default();
        let spec = match loader {
            LoaderKind::Vanilla => LoaderSpec::vanilla(),
            other => LoaderSpec::new(other, Some(LoaderVersion::parse("0.19.5").unwrap())).unwrap(),
        };
        let instance = Instance::new(
            id(),
            InstanceName::parse("Survival").unwrap(),
            GameVersion::parse("1.21.1").unwrap(),
            spec,
            InstanceOrigin::Custom,
        );
        instances.save(&instance).await.unwrap();
        let source = FakeModSource::default()
            .with(
                "iris",
                "Iris Shaders",
                vec![depends(
                    depends(
                        mod_version("iris", "iris-2", "1.21.1", &[LoaderKind::Fabric]),
                        Required,
                        Some("sodium"),
                        None,
                    ),
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
            dir,
            paths,
            instances,
            source,
            fs: FakeFileSystem::default(),
            files: FakePackFiles::default(),
            registry: InMemoryModRegistryStore::default(),
        }
    }

    fn downloader() -> FakeDownloader {
        FakeDownloader::with(["iris-2", "sodium-2"].map(|version| {
            let project = version.split('-').next().unwrap();
            (
                format!("https://cdn.modrinth.com/data/{project}/versions/{version}/{version}.jar"),
                mod_jar(version),
            )
        }))
    }

    fn deps<'a>(
        world: &'a World,
        downloader: &'a FakeDownloader,
    ) -> InstallModDeps<
        'a,
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
            downloader,
            fs: &world.fs,
            files: &world.files,
            registry: &world.registry,
            paths: &world.paths,
        }
    }

    fn mods_dir(world: &World) -> std::path::PathBuf {
        world.paths.game_dir(&id()).join(MODS_DIR)
    }

    #[tokio::test]
    async fn installs_the_mod_and_its_dependencies_verified_into_mods() {
        let world = world(LoaderKind::Fabric).await;
        let downloader = downloader();
        let reporter = RecordingReporter::default();
        let written = install_mod(
            &deps(&world, &downloader),
            &id(),
            "iris",
            &reporter,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read(mods_dir(&world).join("iris-2.jar")).unwrap(),
            mod_jar("iris-2")
        );
        assert_eq!(
            std::fs::read(mods_dir(&world).join("sodium-2.jar")).unwrap(),
            mod_jar("sodium-2")
        );
        // Every download is checked against the catalog's sha512.
        let requests = downloader.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| request.checksum.is_some()));
        assert!(
            requests
                .iter()
                .all(|request| !request.destination.starts_with(mods_dir(&world)))
        );
        assert!(
            reporter
                .updates()
                .iter()
                .all(|update| update.stage == Some(ProgressStage::Mods))
        );

        assert_eq!(written.len(), 2);
        let registry = world.registry.load(&id()).await.unwrap();
        let iris = registry.get("iris").unwrap();
        assert!(iris.requested);
        assert_eq!(iris.title, "Iris Shaders");
        assert_eq!(iris.filename, "iris-2.jar");
        assert_eq!(iris.incompatible, BTreeSet::from(["optifine".to_owned()]));
        let sodium = registry.get("sodium").unwrap();
        assert!(!sodium.requested);
        assert_eq!(sodium.required_by, BTreeSet::from(["iris".to_owned()]));
        assert_eq!(sodium.sha512.len(), 128);
    }

    #[tokio::test]
    async fn marks_an_installed_dependency_as_needed_instead_of_reinstalling_it() {
        let world = world(LoaderKind::Fabric).await;
        let downloader = downloader();
        install_mod(
            &deps(&world, &downloader),
            &id(),
            "sodium",
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        install_mod(
            &deps(&world, &downloader),
            &id(),
            "iris",
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        // Sodium was downloaded once, when the player asked for it.
        assert_eq!(downloader.requests().len(), 2);
        let registry = world.registry.load(&id()).await.unwrap();
        let sodium = registry.get("sodium").unwrap();
        assert!(sodium.requested);
        assert_eq!(sodium.required_by, BTreeSet::from(["iris".to_owned()]));
    }

    #[tokio::test]
    async fn a_refused_plan_writes_nothing() {
        let world = world(LoaderKind::Vanilla).await;
        let downloader = downloader();
        let result = install_mod(
            &deps(&world, &downloader),
            &id(),
            "iris",
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(result, Err(InstallModError::Plan(ModPlanError::NoLoader)));
        assert!(downloader.requests().is_empty());
        assert!(!mods_dir(&world).exists());
        assert_eq!(
            world.registry.load(&id()).await.unwrap(),
            ModRegistry::default()
        );
    }

    #[tokio::test]
    async fn a_cancelled_install_leaves_the_instance_untouched() {
        let world = world(LoaderKind::Fabric).await;
        let downloader = downloader();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = install_mod(
            &deps(&world, &downloader),
            &id(),
            "iris",
            &RecordingReporter::default(),
            &cancel,
        )
        .await;
        assert_eq!(result, Err(InstallModError::Cancelled));
        assert!(!mods_dir(&world).exists());
        assert_eq!(
            world.registry.load(&id()).await.unwrap(),
            ModRegistry::default()
        );
        drop(world.dir);
    }

    #[tokio::test]
    async fn an_unknown_instance_is_reported() {
        let world = world(LoaderKind::Fabric).await;
        let downloader = downloader();
        let other = InstanceId::parse("other").unwrap();
        let result = install_mod(
            &deps(&world, &downloader),
            &other,
            "iris",
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(
            result,
            Err(InstallModError::Instance(
                InstanceRepositoryError::NotFound(other)
            ))
        );
    }
}
