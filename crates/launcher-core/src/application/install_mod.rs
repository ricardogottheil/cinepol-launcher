//! Installs one mod into an instance with the required dependencies it
//! pulls in, and records why each one is there.
//!
//! Every jar is downloaded into a scratch folder and verified against its
//! sha512 there; only once all of them arrived are they moved into `mods/`.
//! A jar already in `mods/` with the same bytes is kept, and a different
//! file under the same name is never overwritten. When moving a jar or
//! saving the registry fails, the jars this install moved are removed again,
//! so a failed or cancelled install leaves `mods/` as it was.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::modpack_files::{release_dir, scratch_dir};
use crate::application::plan_mod_install::plan_mod_install;
use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::integrity::HashAlgorithm;
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
    #[error("mods/{filename} already exists and is another file")]
    FileTaken { filename: String },
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
    if cancel.is_cancelled() {
        return Err(InstallModError::Cancelled);
    }
    let instance = deps.instances.get(id).await?;
    let target = ModTarget {
        game_version: instance.game_version.clone(),
        loader: instance.loader.kind(),
    };
    let mut registry = deps.registry.load(id).await?;
    let plan = plan_mod_install(deps.source, &target, &registry, project).await?;
    let jars = jars(&plan.mods)?;
    let mods_dir = deps.paths.game_dir(id).join(MODS_DIR);
    let adopted = adoptable(deps.files, &mods_dir, &jars).await?;
    if cancel.is_cancelled() {
        return Err(InstallModError::Cancelled);
    }

    let staging = scratch_dir(deps.paths, "mods");
    let placed = write_jars(deps, &mods_dir, &jars, &adopted, &staging, reporter, cancel).await;
    release_dir(deps.fs, &staging).await;
    let placed = placed?;

    for (dependency, needer) in &plan.present {
        if let Some(existing) = registry.get_mut(dependency) {
            existing.required_by.insert(needer.clone());
        }
    }
    let written: Vec<InstalledMod> = jars
        .iter()
        .map(|(planned, file)| record(planned, file))
        .collect();
    for installed in &written {
        registry.insert(installed.clone());
    }
    if let Err(error) = deps.registry.save(id, &registry).await {
        // Jars the registry does not know about would load unseen.
        remove_all(deps.files, &placed).await;
        return Err(error.into());
    }
    Ok(written)
}

/// Each planned mod with the jar it installs. The plan only picks versions
/// that have one; a version without one is reported as not installable.
fn jars(planned: &[PlannedMod]) -> Result<Vec<(&PlannedMod, &ModFile)>, InstallModError> {
    planned
        .iter()
        .map(|planned| match &planned.version.file {
            Some(file) => Ok((planned, file)),
            None => Err(ModPlanError::NoCompatibleVersion {
                title: planned.title.clone(),
            }
            .into()),
        })
        .collect()
}

/// The jars already in `mods/` byte for byte (put there by hand or by a
/// pack), which are kept and recorded instead of downloaded again. A
/// different file under a planned jar's name is never overwritten.
async fn adoptable<P: PackFiles>(
    files: &P,
    mods_dir: &Path,
    jars: &[(&PlannedMod, &ModFile)],
) -> Result<HashSet<usize>, InstallModError> {
    let mut adopted = HashSet::new();
    for (index, (_, file)) in jars.iter().enumerate() {
        match files
            .digest(&mods_dir.join(&file.filename), HashAlgorithm::Sha512)
            .await?
        {
            None => {}
            Some(hash) if hash.eq_ignore_ascii_case(file.sha512.value()) => {
                adopted.insert(index);
            }
            Some(_) => {
                return Err(InstallModError::FileTaken {
                    filename: file.filename.clone(),
                });
            }
        }
    }
    Ok(adopted)
}

/// Downloads every jar not `adopted` into `staging`, verified, then moves
/// them into `mods_dir` and returns the paths written. When a move fails,
/// the jars already moved are removed again.
#[allow(clippy::too_many_arguments)]
async fn write_jars<I, S, D, F, P, M, R>(
    deps: &InstallModDeps<'_, I, S, D, F, P, M>,
    mods_dir: &Path,
    jars: &[(&PlannedMod, &ModFile)],
    adopted: &HashSet<usize>,
    staging: &Path,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<Vec<PathBuf>, InstallModError>
where
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    R: ProgressReporter,
{
    deps.fs.create_dir_all(staging).await?;
    // The catalog's file name is not trusted as a scratch path.
    let scratch = |index: usize| staging.join(format!("{index}.jar"));
    let fresh: Vec<(usize, &ModFile)> = jars
        .iter()
        .enumerate()
        .filter(|(index, _)| !adopted.contains(index))
        .map(|(index, (_, file))| (index, *file))
        .collect();
    let requests = fresh
        .iter()
        .map(|(index, file)| DownloadRequest {
            url: file.url.clone(),
            destination: scratch(*index),
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

    let mut placed = Vec::with_capacity(fresh.len());
    for (index, file) in fresh {
        // A single safe file name: checked when the catalog answer was read.
        let destination = mods_dir.join(&file.filename);
        if let Err(error) = deps.files.place(&scratch(index), &destination).await {
            remove_all(deps.files, &placed).await;
            return Err(error.into());
        }
        placed.push(destination);
    }
    Ok(placed)
}

/// Best-effort removal of jars this install wrote; nothing else was there.
async fn remove_all<P: PackFiles>(files: &P, paths: &[PathBuf]) {
    for path in paths {
        if let Err(error) = files.remove_file(path).await {
            tracing::warn!(path = %path.display(), %error, "could not remove a mod jar after a failed install");
        }
    }
}

fn record(planned: &PlannedMod, file: &ModFile) -> InstalledMod {
    let version = &planned.version;
    InstalledMod {
        project: version.project.clone(),
        title: planned.title.clone(),
        version: version.id.clone(),
        version_number: version.version_number.clone(),
        filename: file.filename.clone(),
        sha512: file.sha512.value().to_owned(),
        requested: planned.requested,
        required_by: planned.required_by.clone(),
        incompatible: version.incompatible_projects().map(str::to_owned).collect(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::instance::{GameVersion, Instance, InstanceName, InstanceOrigin};
    use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
    use crate::domain::mods::{DependencyKind::*, ModRegistry};
    use crate::testing::sha512_hex;
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

    async fn install(
        world: &World,
        downloader: &FakeDownloader,
    ) -> Result<Vec<InstalledMod>, InstallModError> {
        install_mod(
            &deps(world, downloader),
            &id(),
            "iris",
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
    }

    fn jars_in(world: &World) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(mods_dir(world)) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn a_failed_move_removes_the_jars_already_moved() {
        let world = world(LoaderKind::Fabric).await;
        world.files.fail_places_after(1);
        let downloader = downloader();
        let result = install(&world, &downloader).await;
        assert!(
            matches!(result, Err(InstallModError::Files(_))),
            "{result:?}"
        );
        assert!(jars_in(&world).is_empty(), "{:?}", jars_in(&world));
        assert_eq!(world.files.removed().len(), 1);
        assert_eq!(
            world.registry.load(&id()).await.unwrap(),
            ModRegistry::default()
        );
    }

    #[tokio::test]
    async fn a_failed_registry_save_removes_the_jars_it_wrote() {
        let world = world(LoaderKind::Fabric).await;
        world.registry.fail_saves();
        let downloader = downloader();
        let result = install(&world, &downloader).await;
        assert!(
            matches!(result, Err(InstallModError::Files(_))),
            "{result:?}"
        );
        assert!(jars_in(&world).is_empty(), "{:?}", jars_in(&world));
    }

    #[tokio::test]
    async fn never_overwrites_another_file_under_a_planned_name() {
        let world = world(LoaderKind::Fabric).await;
        std::fs::create_dir_all(mods_dir(&world)).unwrap();
        std::fs::write(mods_dir(&world).join("sodium-2.jar"), b"hand-made").unwrap();
        let downloader = downloader();
        let result = install(&world, &downloader).await;
        assert_eq!(
            result,
            Err(InstallModError::FileTaken {
                filename: "sodium-2.jar".into()
            })
        );
        assert!(downloader.requests().is_empty());
        assert_eq!(
            std::fs::read(mods_dir(&world).join("sodium-2.jar")).unwrap(),
            b"hand-made"
        );
        assert_eq!(jars_in(&world), ["sodium-2.jar"]);
    }

    #[tokio::test]
    async fn keeps_and_records_an_identical_jar_already_in_mods() {
        let world = world(LoaderKind::Fabric).await;
        std::fs::create_dir_all(mods_dir(&world)).unwrap();
        std::fs::write(mods_dir(&world).join("sodium-2.jar"), mod_jar("sodium-2")).unwrap();
        let downloader = downloader();
        install(&world, &downloader).await.unwrap();
        // Only Iris was downloaded; Sodium's jar was already the right one.
        let requests = downloader.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.ends_with("iris-2.jar"));
        let registry = world.registry.load(&id()).await.unwrap();
        let sodium = registry.get("sodium").unwrap();
        assert_eq!(sodium.sha512, sha512_hex(&mod_jar("sodium-2")));
        assert_eq!(sodium.required_by, BTreeSet::from(["iris".to_owned()]));
    }

    #[tokio::test]
    async fn a_rollback_never_removes_a_jar_it_did_not_write() {
        let world = world(LoaderKind::Fabric).await;
        std::fs::create_dir_all(mods_dir(&world)).unwrap();
        std::fs::write(mods_dir(&world).join("sodium-2.jar"), mod_jar("sodium-2")).unwrap();
        world.registry.fail_saves();
        let downloader = downloader();
        assert!(install(&world, &downloader).await.is_err());
        assert_eq!(jars_in(&world), ["sodium-2.jar"]);
    }

    #[tokio::test]
    async fn a_planned_version_without_a_jar_is_refused() {
        let mut world = world(LoaderKind::Fabric).await;
        let mut bare = mod_version("lithium", "lithium-2", "1.21.1", &[LoaderKind::Fabric]);
        bare.file = None;
        world.source = FakeModSource::default().with("lithium", "Lithium", vec![bare]);
        let downloader = downloader();
        let result = install_mod(
            &deps(&world, &downloader),
            &id(),
            "lithium",
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;
        assert!(result.is_err());
        assert!(downloader.requests().is_empty());
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
