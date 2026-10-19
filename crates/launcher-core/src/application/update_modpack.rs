//! Updates an installed modpack in place, from the Modrinth catalog or from a
//! local `.mrpack` file, keeping the player's changes.

use std::path::Path;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::modpack_files::{
    PackSyncDeps, PackSyncError, SyncSummary, download_pack, release_dir, scratch_dir, sync_pack,
};
use crate::domain::instance::{
    Instance, InstanceId, InstanceOrigin, InstanceRepository, InstanceRepositoryError,
};
use crate::domain::modpack::{
    ModpackRef, ModpackSource, ModpackSourceError, ModpackSourceKind, MrpackArchive, PackFileError,
    PackFiles, PackManifestStore,
};
use crate::domain::ports::{Downloader, FileSystem, ProgressReporter};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateModpackError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error("instance {0} was not installed from the Modrinth catalog")]
    NotFromCatalog(InstanceId),
    #[error(transparent)]
    Source(#[from] ModpackSourceError),
    #[error("{0:?} modpacks cannot be updated yet")]
    UnsupportedSource(ModpackSourceKind),
    #[error("modpack version {0} was not found")]
    VersionNotFound(String),
    #[error("modpack version {0} has no .mrpack file")]
    NoPackFile(String),
    #[error("modpack {0} has no installable version")]
    NoInstallableVersion(String),
    #[error(transparent)]
    Pack(PackSyncError),
    #[error("the modpack update was cancelled")]
    Cancelled,
}

impl From<PackSyncError> for UpdateModpackError {
    fn from(error: PackSyncError) -> Self {
        match error {
            PackSyncError::Cancelled => Self::Cancelled,
            other => Self::Pack(other),
        }
    }
}

impl From<PackFileError> for UpdateModpackError {
    fn from(error: PackFileError) -> Self {
        Self::Pack(PackSyncError::Files(error))
    }
}

/// Where the new pack version comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateInput<'a> {
    /// A version of the instance's Modrinth project; the newest one with a
    /// `.mrpack` file when `None`.
    Catalog { version: Option<&'a str> },
    /// A `.mrpack` file on disk, for any instance.
    File(&'a Path),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// The instance already has the target catalog version; nothing changed.
    UpToDate { version: String },
    Updated {
        instance: Box<Instance>,
        files: SyncSummary,
    },
}

/// Everything the modpack updater talks to.
pub struct UpdateModpackDeps<'a, I, C, A, D, F, P, M> {
    pub instances: &'a I,
    pub source: &'a C,
    pub pack: PackSyncDeps<'a, A, D, F, P, M>,
}

/// Only pack files that are still intact are replaced or removed; mods the
/// player added and configs the player changed are kept. The instance's game
/// version, loader and origin follow the new pack.
///
/// Updating a catalog instance from a local file turns it into a
/// [`InstanceOrigin::Custom`] one: its files no longer match a catalog
/// version, so a later catalog update must not trust the recorded one.
pub async fn update_modpack<I, C, A, D, F, P, M, R>(
    id: &InstanceId,
    input: UpdateInput<'_>,
    deps: &UpdateModpackDeps<'_, I, C, A, D, F, P, M>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<UpdateOutcome, UpdateModpackError>
where
    I: InstanceRepository,
    C: ModpackSource,
    A: MrpackArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let instance = deps.instances.get(id).await?;
    let wanted = match input {
        UpdateInput::File(archive) => {
            return update_from(
                archive,
                InstanceOrigin::Custom,
                instance,
                deps,
                reporter,
                cancel,
            )
            .await;
        }
        UpdateInput::Catalog { version } => version,
    };
    let installed = match &instance.origin {
        InstanceOrigin::Modpack(pack) if pack.source == ModpackSourceKind::Modrinth => pack.clone(),
        _ => return Err(UpdateModpackError::NotFromCatalog(instance.id)),
    };
    if deps.source.kind() != ModpackSourceKind::Modrinth {
        return Err(UpdateModpackError::UnsupportedSource(deps.source.kind()));
    }
    let versions = deps.source.versions(&installed.project).await?;
    let target = match wanted {
        Some(wanted) => versions
            .iter()
            .find(|candidate| candidate.id == wanted)
            .ok_or_else(|| UpdateModpackError::VersionNotFound(wanted.to_owned()))?,
        None => versions
            .iter()
            .find(|candidate| candidate.pack_file.is_some())
            .ok_or_else(|| UpdateModpackError::NoInstallableVersion(installed.project.clone()))?,
    };
    if target.id == installed.version {
        return Ok(UpdateOutcome::UpToDate {
            version: target.id.clone(),
        });
    }
    let file = target
        .pack_file
        .as_ref()
        .ok_or_else(|| UpdateModpackError::NoPackFile(target.id.clone()))?;
    let origin = InstanceOrigin::Modpack(ModpackRef {
        version: target.id.clone(),
        ..installed
    });

    let dir = scratch_dir(deps.pack.paths, "download");
    let result = async {
        let archive = download_pack(deps.pack.downloader, file, &dir, reporter, cancel).await?;
        update_from(&archive, origin, instance, deps, reporter, cancel).await
    }
    .await;
    release_dir(deps.pack.fs, &dir).await;
    result
}

async fn update_from<I, C, A, D, F, P, M, R>(
    archive: &Path,
    origin: InstanceOrigin,
    instance: Instance,
    deps: &UpdateModpackDeps<'_, I, C, A, D, F, P, M>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<UpdateOutcome, UpdateModpackError>
where
    I: InstanceRepository,
    A: MrpackArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let index = deps.pack.archive.read_index(archive).await?;
    let previous = deps.pack.manifests.load(&instance.id).await?;
    let files = sync_pack(
        &deps.pack,
        &instance.id,
        archive,
        &index,
        &previous,
        reporter,
        cancel,
    )
    .await?;

    // Re-read: the player may have edited the settings during the sync.
    let mut instance = deps.instances.get(&instance.id).await?;
    let before = instance.clone();
    instance.game_version = index.game_version;
    instance.loader = index.loader;
    instance.origin = origin;
    if instance != before {
        deps.instances.save(&instance).await?;
    }
    Ok(UpdateOutcome::Updated {
        instance: Box::new(instance),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::install_modpack::{
        InstallModpackDeps, InstallModpackRequest, PackInput, install_modpack,
    };
    use crate::application::modpack_files::fixtures::*;
    use crate::domain::modpack::sync::PackManifest;
    use crate::testing::{
        FakeFileSystem, FakeModpackSource, FakePackFiles, InMemoryInstanceRepository,
        InMemoryPackManifestStore, RecordingReporter, sha1_hex,
    };

    const ID: &str = "fabulously-optimized";
    const V1_FILE: &str = "/packs/fo-v1.mrpack";
    const V2_FILE: &str = "/packs/fo-v2.mrpack";

    fn v1(key: String) -> Pack {
        let files = vec![
            entry("v1", "mods/sodium.jar", "required"),
            entry("v1", "mods/old.jar", "required"),
            entry("v1", "config/sodium.json", "required"),
        ];
        (key, index("v1", "26.2", "0.19.5", files), vec![])
    }

    fn v2(key: String) -> Pack {
        let files = vec![
            entry("v2", "mods/sodium.jar", "required"),
            entry("v2", "mods/new.jar", "required"),
            entry("v2", "config/sodium.json", "required"),
        ];
        (key, index("v2", "26.3", "0.20.0", files), vec![])
    }

    /// A world whose catalog serves v3 (no pack), v2 and v1, and that also
    /// has both versions as local files.
    fn world() -> World {
        let mut world = World::new(vec![
            v1(pack_url("v1")),
            v2(pack_url("v2")),
            v1(V1_FILE.into()),
            v2(V2_FILE.into()),
        ]);
        world.source.versions.insert(
            "fo".into(),
            vec![
                catalog_version("v3", false),
                catalog_version("v2", true),
                catalog_version("v1", true),
            ],
        );
        world
    }

    async fn install(world: &World, pack: PackInput<'_>) -> Instance {
        let deps = InstallModpackDeps {
            instances: &world.instances,
            servers: &crate::testing::InMemoryServerListStore::default(),
            source: &world.source,
            pack: world.pack_deps(&world.downloader),
        };
        let request = InstallModpackRequest {
            pack,
            name: None,
            seed_server: None,
            claim: None,
        };
        install_modpack(
            request,
            &deps,
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .instance
    }

    async fn install_catalog(world: &World, version: &str) -> Instance {
        install(
            world,
            PackInput::Catalog {
                project: "fo",
                version,
            },
        )
        .await
    }

    type Deps<'a, D> = UpdateModpackDeps<
        'a,
        InMemoryInstanceRepository,
        FakeModpackSource,
        TestArchive,
        D,
        FakeFileSystem,
        FakePackFiles,
        InMemoryPackManifestStore,
    >;

    fn deps<'a, D>(world: &'a World, downloader: &'a D) -> Deps<'a, D> {
        UpdateModpackDeps {
            instances: &world.instances,
            source: &world.source,
            pack: world.pack_deps(downloader),
        }
    }

    async fn update(
        world: &World,
        input: UpdateInput<'_>,
    ) -> Result<UpdateOutcome, UpdateModpackError> {
        update_modpack(
            &InstanceId::parse(ID).unwrap(),
            input,
            &deps(world, &world.downloader),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
    }

    fn newest() -> UpdateInput<'static> {
        UpdateInput::Catalog { version: None }
    }

    fn content(version: &str, path: &str) -> Vec<u8> {
        url(version, path).into_bytes()
    }

    #[tokio::test]
    async fn keeps_player_changes_and_replaces_only_intact_pack_files() {
        let world = world();
        install_catalog(&world, "v1").await;
        world.write_game_file(ID, "config/sodium.json", b"player settings");
        world.write_game_file(ID, "mods/mine.jar", b"player mod");

        let outcome = update(&world, newest()).await.unwrap();

        let UpdateOutcome::Updated { instance, files } = outcome else {
            panic!("expected an update, got {outcome:?}");
        };
        assert_eq!(
            files,
            SyncSummary {
                written: 2,
                unchanged: 0,
                kept: 1,
                removed: 1,
            }
        );
        let read = |path| world.read_game_file(ID, path);
        assert_eq!(
            read("mods/sodium.jar").unwrap(),
            content("v2", "mods/sodium.jar")
        );
        assert_eq!(read("mods/new.jar").unwrap(), content("v2", "mods/new.jar"));
        assert_eq!(read("config/sodium.json").unwrap(), b"player settings");
        assert_eq!(read("mods/mine.jar").unwrap(), b"player mod");
        assert!(read("mods/old.jar").is_none());

        assert_eq!(instance.game_version.as_str(), "26.3");
        assert_eq!(instance.loader.version().unwrap().as_str(), "0.20.0");
        assert_eq!(
            instance.origin,
            InstanceOrigin::Modpack(ModpackRef {
                source: ModpackSourceKind::Modrinth,
                project: "fo".into(),
                version: "v2".into(),
            })
        );
        assert_eq!(world.instances.get(&instance.id).await.unwrap(), *instance);

        let manifest = world.manifests.load(&instance.id).await.unwrap();
        let recorded: Vec<(&str, &str)> = manifest
            .files
            .iter()
            .map(|(path, sha1)| (path.as_str(), sha1.as_str()))
            .collect();
        assert_eq!(
            recorded,
            [
                (
                    "mods/new.jar",
                    sha1_hex(&content("v2", "mods/new.jar")).as_str()
                ),
                (
                    "mods/sodium.jar",
                    sha1_hex(&content("v2", "mods/sodium.jar")).as_str()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn reports_an_instance_that_is_already_up_to_date() {
        let world = world();
        install_catalog(&world, "v2").await;
        let downloads = world.downloader.requests().len();
        assert_eq!(
            update(&world, newest()).await,
            Ok(UpdateOutcome::UpToDate {
                version: "v2".into()
            })
        );
        assert_eq!(world.downloader.requests().len(), downloads);
    }

    #[tokio::test]
    async fn updates_to_a_chosen_catalog_version() {
        let world = world();
        install_catalog(&world, "v2").await;
        let outcome = update(
            &world,
            UpdateInput::Catalog {
                version: Some("v1"),
            },
        )
        .await
        .unwrap();
        let UpdateOutcome::Updated { instance, .. } = outcome else {
            panic!("expected an update, got {outcome:?}");
        };
        assert_eq!(instance.game_version.as_str(), "26.2");
        assert_eq!(
            update(
                &world,
                UpdateInput::Catalog {
                    version: Some("v3")
                }
            )
            .await,
            Err(UpdateModpackError::NoPackFile("v3".into()))
        );
        assert_eq!(
            update(
                &world,
                UpdateInput::Catalog {
                    version: Some("v9")
                }
            )
            .await,
            Err(UpdateModpackError::VersionNotFound("v9".into()))
        );
    }

    #[tokio::test]
    async fn a_catalog_update_needs_a_catalog_instance() {
        let world = world();
        install(&world, PackInput::File(Path::new(V1_FILE))).await;
        assert_eq!(
            update(&world, newest()).await,
            Err(UpdateModpackError::NotFromCatalog(
                InstanceId::parse(ID).unwrap()
            ))
        );
    }

    #[tokio::test]
    async fn updates_any_instance_from_a_local_file() {
        let world = world();
        install(&world, PackInput::File(Path::new(V1_FILE))).await;
        let outcome = update(&world, UpdateInput::File(Path::new(V2_FILE)))
            .await
            .unwrap();
        let UpdateOutcome::Updated { instance, files } = outcome else {
            panic!("expected an update, got {outcome:?}");
        };
        assert_eq!(files.written, 3);
        assert_eq!(files.removed, 1);
        assert_eq!(instance.origin, InstanceOrigin::Custom);
        assert_eq!(instance.game_version.as_str(), "26.3");
        assert_eq!(
            world.read_game_file(ID, "mods/new.jar").unwrap(),
            content("v2", "mods/new.jar")
        );
    }

    #[tokio::test]
    async fn a_local_update_detaches_a_catalog_instance() {
        let world = world();
        install_catalog(&world, "v1").await;
        let outcome = update(&world, UpdateInput::File(Path::new(V2_FILE)))
            .await
            .unwrap();
        let UpdateOutcome::Updated { instance, .. } = outcome else {
            panic!("expected an update, got {outcome:?}");
        };
        assert_eq!(instance.origin, InstanceOrigin::Custom);
    }

    #[tokio::test]
    async fn a_failed_sync_keeps_the_previous_manifest_and_instance() {
        let world = world();
        let before = install(&world, PackInput::File(Path::new(V1_FILE))).await;
        let id = before.id.clone();
        let manifest: PackManifest = world.manifests.load(&id).await.unwrap();

        let result = update_modpack(
            &id,
            UpdateInput::File(Path::new(V2_FILE)),
            &deps(&world, &FailingDownloader),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;

        assert!(matches!(
            result,
            Err(UpdateModpackError::Pack(PackSyncError::Download(_)))
        ));
        assert_eq!(world.manifests.load(&id).await.unwrap(), manifest);
        assert_eq!(world.instances.get(&id).await.unwrap(), before);
    }

    #[tokio::test]
    async fn cancellation_stops_before_downloading() {
        let world = world();
        install_catalog(&world, "v1").await;
        let downloads = world.downloader.requests().len();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = update_modpack(
            &InstanceId::parse(ID).unwrap(),
            newest(),
            &deps(&world, &world.downloader),
            &RecordingReporter::default(),
            &cancel,
        )
        .await;
        assert_eq!(result, Err(UpdateModpackError::Cancelled));
        assert_eq!(world.downloader.requests().len(), downloads);
    }

    /// Lets the pack download through, then writes one file of the next batch and fails.
    struct InterruptedDownloader<'a> {
        inner: &'a crate::testing::FakeDownloader,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl crate::domain::ports::Downloader for InterruptedDownloader<'_> {
        async fn download_all<R: crate::domain::ports::ProgressReporter>(
            &self,
            requests: Vec<crate::domain::ports::DownloadRequest>,
            reporter: &R,
            cancel: &CancellationToken,
        ) -> Result<crate::domain::ports::DownloadSummary, crate::domain::ports::DownloadError>
        {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                return self.inner.download_all(requests, reporter, cancel).await;
            }
            let first = requests.into_iter().take(1).collect();
            self.inner.download_all(first, reporter, cancel).await?;
            Err(crate::domain::ports::DownloadError::Http {
                url: "https://cdn.modrinth.com/next".into(),
                reason: "connection reset".into(),
            })
        }
    }

    #[tokio::test]
    async fn an_interrupted_download_leaves_the_game_folder_untouched() {
        let world = world();
        install_catalog(&world, "v1").await;
        let id = InstanceId::parse(ID).unwrap();
        let before = world.manifests.manifests.lock().unwrap()[&id].clone();
        let downloader = InterruptedDownloader {
            inner: &world.downloader,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };

        let result = update_modpack(
            &id,
            newest(),
            &deps(&world, &downloader),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;

        assert!(matches!(result, Err(UpdateModpackError::Pack(_))));
        for path in ["mods/sodium.jar", "config/sodium.json"] {
            assert_eq!(
                world.read_game_file(ID, path).unwrap(),
                content("v1", path),
                "{path}"
            );
        }
        assert_eq!(world.read_game_file(ID, "mods/new.jar"), None);
        assert_eq!(world.manifests.manifests.lock().unwrap()[&id], before);
        assert_eq!(
            world
                .instances
                .get(&id)
                .await
                .unwrap()
                .game_version
                .as_str(),
            "26.2"
        );
    }

    /// Saves a settings edit (a rename) on the second download call: the first
    /// one fetches the pack, the second its files, so the edit lands mid-sync.
    struct EditingDownloader<'a> {
        inner: &'a crate::testing::FakeDownloader,
        instances: &'a InMemoryInstanceRepository,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl crate::domain::ports::Downloader for EditingDownloader<'_> {
        async fn download_all<R: crate::domain::ports::ProgressReporter>(
            &self,
            requests: Vec<crate::domain::ports::DownloadRequest>,
            reporter: &R,
            cancel: &CancellationToken,
        ) -> Result<crate::domain::ports::DownloadSummary, crate::domain::ports::DownloadError>
        {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                let id = InstanceId::parse(ID).unwrap();
                let mut instance = self.instances.get(&id).await.unwrap();
                instance.name = crate::domain::instance::InstanceName::parse("Renamed").unwrap();
                self.instances.save(&instance).await.unwrap();
            }
            self.inner.download_all(requests, reporter, cancel).await
        }
    }

    #[tokio::test]
    async fn a_settings_edit_saved_during_an_update_is_kept() {
        let world = world();
        install_catalog(&world, "v1").await;
        let id = InstanceId::parse(ID).unwrap();
        let downloader = EditingDownloader {
            inner: &world.downloader,
            instances: &world.instances,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let result = update_modpack(
            &id,
            newest(),
            &deps(&world, &downloader),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;
        assert!(
            matches!(result, Ok(UpdateOutcome::Updated { .. })),
            "{result:?}"
        );
        let stored = world.instances.get(&id).await.unwrap();
        assert_eq!(stored.game_version.as_str(), "26.3");
        assert_eq!(
            stored.name.as_str(),
            "Renamed",
            "the update overwrote the settings edit"
        );
    }
}
