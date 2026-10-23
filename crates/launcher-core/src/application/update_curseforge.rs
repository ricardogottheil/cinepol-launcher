//! Updates an instance installed from the CurseForge catalog to another file
//! of its modpack project.

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::install_curseforge::{
    InstallCurseForgeError, read_curseforge, sync_curseforge,
};
use crate::application::modpack_files::{
    FileSyncDeps, PackSyncError, SyncSummary, download_pack, release_dir, scratch_dir,
};
use crate::domain::instance::{
    Instance, InstanceId, InstanceOrigin, InstanceRepository, InstanceRepositoryError,
};
use crate::domain::modpack::sync::ManualFile;
use crate::domain::modpack::{
    CurseForgeArchive, CurseForgeLookup, ModpackRef, ModpackSource, ModpackSourceError,
    ModpackSourceKind, PackFileError, PackFiles, PackManifestStore,
};
use crate::domain::ports::{Downloader, FileSystem, ProgressReporter};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateCurseForgeError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error("instance {0} was not installed from the CurseForge catalog")]
    NotFromCatalog(InstanceId),
    #[error(transparent)]
    Source(#[from] ModpackSourceError),
    #[error("modpack file {0} was not found")]
    VersionNotFound(String),
    #[error("modpack file {0} cannot be downloaded by the launcher")]
    NotInstallable(String),
    #[error("modpack {0} has no installable file")]
    NoInstallableVersion(String),
    #[error(transparent)]
    Pack(PackSyncError),
    #[error("the modpack update was cancelled")]
    Cancelled,
}

impl From<PackSyncError> for UpdateCurseForgeError {
    fn from(error: PackSyncError) -> Self {
        match error {
            PackSyncError::Cancelled => Self::Cancelled,
            other => Self::Pack(other),
        }
    }
}

impl From<PackFileError> for UpdateCurseForgeError {
    fn from(error: PackFileError) -> Self {
        Self::Pack(PackSyncError::Files(error))
    }
}

impl From<InstallCurseForgeError> for UpdateCurseForgeError {
    fn from(error: InstallCurseForgeError) -> Self {
        match error {
            InstallCurseForgeError::Source(error) => Self::Source(error),
            InstallCurseForgeError::VersionNotFound(file) => Self::VersionNotFound(file),
            InstallCurseForgeError::NotInstallable(file) => Self::NotInstallable(file),
            InstallCurseForgeError::Pack(error) => Self::Pack(error),
            InstallCurseForgeError::Cancelled => Self::Cancelled,
            // Reading a pack never creates or claims an instance.
            other @ (InstallCurseForgeError::Create(_) | InstallCurseForgeError::Busy(_)) => {
                Self::Pack(PackSyncError::Files(PackFileError::Source(
                    other.to_string(),
                )))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCurseForgeOutcome {
    /// The instance already has the target file; nothing changed.
    UpToDate { version: String },
    Updated {
        instance: Box<Instance>,
        files: SyncSummary,
        /// Files the player must download by hand to complete the pack.
        pending: Vec<ManualFile>,
    },
}

/// Everything the CurseForge updater talks to.
pub struct UpdateCurseForgeDeps<'a, I, C, A, D, F, P, M> {
    pub instances: &'a I,
    pub source: &'a C,
    pub archive: &'a A,
    pub files: FileSyncDeps<'a, D, F, P, M>,
}

/// Updates to `file`, or to the newest downloadable file when `None`. Only
/// pack files that are still intact are replaced or removed; mods the player
/// added and configs the player changed are kept. The instance's game
/// version, loader and origin follow the new file.
pub async fn update_curseforge<I, C, A, D, F, P, M, R>(
    id: &InstanceId,
    file: Option<&str>,
    deps: &UpdateCurseForgeDeps<'_, I, C, A, D, F, P, M>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<UpdateCurseForgeOutcome, UpdateCurseForgeError>
where
    I: InstanceRepository,
    C: ModpackSource + CurseForgeLookup,
    A: CurseForgeArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let instance = deps.instances.get(id).await?;
    let installed = match &instance.origin {
        InstanceOrigin::Modpack(pack) if pack.source == ModpackSourceKind::CurseForge => {
            pack.clone()
        }
        _ => return Err(UpdateCurseForgeError::NotFromCatalog(instance.id)),
    };
    let versions = deps.source.versions(&installed.project).await?;
    let target = match file {
        Some(wanted) => versions
            .iter()
            .find(|candidate| candidate.id == wanted)
            .ok_or_else(|| UpdateCurseForgeError::VersionNotFound(wanted.to_owned()))?,
        None => versions
            .iter()
            .find(|candidate| candidate.pack_file.is_some())
            .ok_or_else(|| {
                UpdateCurseForgeError::NoInstallableVersion(installed.project.clone())
            })?,
    };
    if target.id == installed.version {
        return Ok(UpdateCurseForgeOutcome::UpToDate {
            version: target.id.clone(),
        });
    }
    let pack_file = target
        .pack_file
        .as_ref()
        .ok_or_else(|| UpdateCurseForgeError::NotInstallable(target.id.clone()))?;
    let origin = InstanceOrigin::Modpack(ModpackRef {
        version: target.id.clone(),
        ..installed
    });

    let dir = scratch_dir(deps.files.paths, "download");
    let result = async {
        let archive =
            download_pack(deps.files.downloader, pack_file, &dir, reporter, cancel).await?;
        let contents = read_curseforge(deps.source, deps.archive, &archive).await?;
        let previous = deps.files.manifests.load(&instance.id).await?;
        let synced = sync_curseforge(
            deps.archive,
            &deps.files,
            &instance.id,
            &archive,
            &contents,
            &previous,
            reporter,
            cancel,
        )
        .await?;

        // Re-read: the player may have edited the settings during the sync.
        let mut current = deps.instances.get(&instance.id).await?;
        let before = current.clone();
        current.game_version = contents.pack.game_version;
        current.loader = contents.pack.loader;
        current.origin = origin;
        if current != before {
            deps.instances.save(&current).await?;
        }
        Ok(UpdateCurseForgeOutcome::Updated {
            instance: Box::new(current),
            files: synced.summary,
            pending: synced.pending,
        })
    }
    .await;
    release_dir(deps.files.fs, &dir).await;
    result
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::application::install_curseforge::fixtures::*;
    use crate::application::install_curseforge::{
        CurseForgeDeps, CurseForgeInput, InstallCurseForgeRequest, install_curseforge,
    };
    use crate::application::modpack_files::fixtures::{World, rel};
    use crate::domain::loader::LoaderKind;
    use crate::domain::modpack::StagedFile;
    use crate::domain::modpack::curseforge::RawManifest;
    use crate::testing::{
        FakeCurseForge, FakeCurseForgeArchive, FakeDownloader, RecordingReporter,
    };

    const ID: &str = "fabulously-optimized";

    struct ByContent(FakeCurseForgeArchive);

    impl ByContent {
        fn key(&self, archive: &Path) -> std::path::PathBuf {
            std::fs::read_to_string(archive)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| archive.to_path_buf())
        }
    }

    impl CurseForgeArchive for ByContent {
        async fn read_manifest(&self, archive: &Path) -> Result<RawManifest, PackFileError> {
            self.0.read_manifest(&self.key(archive)).await
        }

        async fn extract_overrides(
            &self,
            archive: &Path,
            folder: &str,
            staging: &Path,
        ) -> Result<Vec<StagedFile>, PackFileError> {
            self.0
                .extract_overrides(&self.key(archive), folder, staging)
                .await
        }
    }

    struct Setup {
        world: World,
        source: FakeCurseForge,
        archive: ByContent,
        downloader: FakeDownloader,
    }

    /// v1 (file 100): sodium + old mod; v2 (file 200, newer, Quilt on 26.3):
    /// sodium + entity culling (manual), old mod gone.
    fn setup() -> Setup {
        let files = vec![
            cf_file(10, 1, "sodium.jar", true),
            cf_file(11, 3, "old.zip", true),
            cf_file(20, 2, "entityculling.jar", false),
        ];
        let source = catalog(
            vec![
                pack_version("200", "fo-v2.zip"),
                pack_version("100", "fo-v1.zip"),
            ],
            files,
        );
        let archive = ByContent(archive(vec![
            (
                url("fo-v1.zip"),
                manifest("1", "26.2", "fabric-0.19.5", &[(1, 10), (3, 11)]),
                vec![],
            ),
            (
                url("fo-v2.zip"),
                manifest("2", "26.3", "quilt-0.26.0", &[(1, 10), (2, 20)]),
                vec![("config/new.json", b"{}")],
            ),
        ]));
        let names = ["fo-v1.zip", "fo-v2.zip", "sodium.jar", "old.zip"];
        let downloader = FakeDownloader::with(names.map(|n| (url(n), url(n).into_bytes())));
        Setup {
            world: World::new(vec![]),
            source,
            archive,
            downloader,
        }
    }

    fn files<'a>(
        s: &'a Setup,
    ) -> FileSyncDeps<
        'a,
        FakeDownloader,
        crate::testing::FakeFileSystem,
        crate::testing::FakePackFiles,
        crate::testing::InMemoryPackManifestStore,
    > {
        FileSyncDeps {
            downloader: &s.downloader,
            fs: &s.world.fs,
            files: &s.world.files,
            manifests: &s.world.manifests,
            paths: &s.world.paths,
        }
    }

    async fn install_v1(s: &Setup) -> Instance {
        let deps = CurseForgeDeps {
            instances: &s.world.instances,
            servers: &s.world.servers,
            source: &s.source,
            archive: &s.archive,
            files: files(s),
        };
        install_curseforge(
            InstallCurseForgeRequest {
                pack: CurseForgeInput::Catalog {
                    project: PROJECT,
                    file: "100",
                },
                name: None,
                seed_server: None,
                claim: None,
            },
            &deps,
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .instance
    }

    async fn update(
        s: &Setup,
        file: Option<&str>,
    ) -> Result<UpdateCurseForgeOutcome, UpdateCurseForgeError> {
        let deps = UpdateCurseForgeDeps {
            instances: &s.world.instances,
            source: &s.source,
            archive: &s.archive,
            files: files(s),
        };
        update_curseforge(
            &InstanceId::parse(ID).unwrap(),
            file,
            &deps,
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
    }

    #[tokio::test]
    async fn updates_to_the_newest_file_and_follows_its_game_and_loader() {
        let s = setup();
        install_v1(&s).await;
        s.world.write_game_file(ID, "mods/mine.jar", b"player mod");

        let UpdateCurseForgeOutcome::Updated {
            instance, pending, ..
        } = update(&s, None).await.unwrap()
        else {
            panic!("expected an update");
        };
        assert_eq!(instance.game_version.as_str(), "26.3");
        assert_eq!(instance.loader.kind(), LoaderKind::Quilt);
        assert_eq!(
            instance.origin,
            InstanceOrigin::Modpack(ModpackRef {
                source: ModpackSourceKind::CurseForge,
                project: PROJECT.into(),
                version: "200".into(),
            })
        );
        // The saved instance matches what was returned.
        assert_eq!(
            s.world.instances.get(&instance.id).await.unwrap(),
            *instance
        );

        assert!(s.world.read_game_file(ID, "mods/sodium.jar").is_some());
        assert!(
            s.world
                .read_game_file(ID, "resourcepacks/old.zip")
                .is_none(),
            "dropped by v2"
        );
        assert_eq!(
            s.world.read_game_file(ID, "config/new.json").unwrap(),
            b"{}"
        );
        assert_eq!(
            s.world.read_game_file(ID, "mods/mine.jar").unwrap(),
            b"player mod"
        );
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].path, rel("mods/entityculling.jar"));
        let manifest = s.world.manifests.load(&instance.id).await.unwrap();
        assert_eq!(manifest.pending, pending);
    }

    #[tokio::test]
    async fn reports_up_to_date_and_refuses_unknown_or_blocked_files() {
        let mut s = setup();
        install_v1(&s).await;
        assert_eq!(
            update(&s, Some("100")).await,
            Ok(UpdateCurseForgeOutcome::UpToDate {
                version: "100".into()
            })
        );
        assert_eq!(
            update(&s, Some("999")).await,
            Err(UpdateCurseForgeError::VersionNotFound("999".into()))
        );
        let mut blocked = pack_version("200", "fo-v2.zip");
        blocked.pack_file = None;
        s.source.versions.insert(
            PROJECT.into(),
            vec![blocked.clone(), pack_version("100", "fo-v1.zip")],
        );
        assert_eq!(
            update(&s, Some("200")).await,
            Err(UpdateCurseForgeError::NotInstallable("200".into()))
        );
        // The newest downloadable file is v1, which is installed.
        assert_eq!(
            update(&s, None).await,
            Ok(UpdateCurseForgeOutcome::UpToDate {
                version: "100".into()
            })
        );
        s.source.versions.insert(PROJECT.into(), vec![blocked]);
        assert_eq!(
            update(&s, None).await,
            Err(UpdateCurseForgeError::NoInstallableVersion(PROJECT.into()))
        );
    }

    #[tokio::test]
    async fn only_curseforge_catalog_instances_update() {
        let s = setup();
        let mut instance = install_v1(&s).await;
        instance.origin = InstanceOrigin::Custom;
        s.world.instances.save(&instance).await.unwrap();
        assert_eq!(
            update(&s, None).await,
            Err(UpdateCurseForgeError::NotFromCatalog(instance.id.clone()))
        );
        instance.origin = InstanceOrigin::Modpack(ModpackRef {
            source: ModpackSourceKind::Modrinth,
            project: "AANobbMI".into(),
            version: "x".into(),
        });
        s.world.instances.save(&instance).await.unwrap();
        assert_eq!(
            update(&s, None).await,
            Err(UpdateCurseForgeError::NotFromCatalog(instance.id))
        );
    }
}
