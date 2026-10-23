//! Installs a CurseForge modpack, from the catalog or a local zip, into a
//! new instance.
//!
//! The zip's `manifest.json` lists project and file ids. Their files are
//! looked up in batches and downloaded from CurseForge's CDN, verified by
//! sha1 and size; files whose author forbids third-party distribution become
//! manual downloads, stored in the manifest like packwiz's CurseForge-mode
//! files. The zip's overrides folder is copied last. Like `install_modpack`,
//! this does not install Minecraft or the loader.

use std::collections::BTreeSet;
use std::path::Path;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::create_instance::{CreateInstanceError, NewInstance, create_instance};
use crate::application::install_modpack::{ClaimInstance, discard_instance};
use crate::application::modpack_files::{
    FileSyncDeps, PackSyncError, SyncSummary, SyncedFiles, download_pack, release_dir, scratch_dir,
    sync_into,
};
use crate::domain::instance::{Instance, InstanceId, InstanceOrigin, InstanceRepository};
use crate::domain::modpack::curseforge::{CurseForgePack, ResolvedFiles, resolve_files};
use crate::domain::modpack::sync::{EntrySource, ManualFile, PackEntry, PackManifest};
use crate::domain::modpack::{
    CurseForgeArchive, CurseForgeLookup, ModpackRef, ModpackSource, ModpackSourceError,
    ModpackSourceKind, PackFileError, PackFiles, PackManifestStore,
};
use crate::domain::ports::{Downloader, FileSystem, ProgressReporter};
use crate::domain::server_list::{ServerEntry, ServerListStore};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstallCurseForgeError {
    #[error(transparent)]
    Source(#[from] ModpackSourceError),
    #[error("modpack file {0} was not found")]
    VersionNotFound(String),
    #[error("modpack file {0} cannot be downloaded by the launcher")]
    NotInstallable(String),
    #[error(transparent)]
    Create(#[from] CreateInstanceError),
    #[error(transparent)]
    Pack(PackSyncError),
    #[error("the modpack installation was cancelled")]
    Cancelled,
    #[error("instance {0} is busy")]
    Busy(InstanceId),
}

impl From<PackSyncError> for InstallCurseForgeError {
    fn from(error: PackSyncError) -> Self {
        match error {
            PackSyncError::Cancelled => Self::Cancelled,
            other => Self::Pack(other),
        }
    }
}

impl From<PackFileError> for InstallCurseForgeError {
    fn from(error: PackFileError) -> Self {
        Self::Pack(PackSyncError::Files(error))
    }
}

/// Where the pack comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurseForgeInput<'a> {
    /// A file of a CurseForge modpack project, by ids.
    Catalog { project: &'a str, file: &'a str },
    /// A CurseForge pack zip on disk.
    File(&'a Path),
}

pub struct InstallCurseForgeRequest<'a> {
    pub pack: CurseForgeInput<'a>,
    /// The instance name; the pack's name when `None` or blank.
    pub name: Option<&'a str>,
    /// Put first in the new instance's `servers.dat` (the CINEPOL server).
    pub seed_server: Option<ServerEntry>,
    /// Called once the new instance exists and before any file is written.
    /// Returning `false` aborts with [`InstallCurseForgeError::Busy`].
    pub claim: Option<ClaimInstance<'a>>,
}

/// Everything the CurseForge installer talks to.
pub struct CurseForgeDeps<'a, I, S, C, A, D, F, P, M> {
    pub instances: &'a I,
    pub servers: &'a S,
    pub source: &'a C,
    pub archive: &'a A,
    pub files: FileSyncDeps<'a, D, F, P, M>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledCurseForge {
    pub instance: Instance,
    pub files: SyncSummary,
    /// Files the player must download by hand to complete the pack.
    pub pending: Vec<ManualFile>,
}

/// A catalog pack gets a [`InstanceOrigin::Modpack`] origin so it can be
/// updated later; a local zip gets [`InstanceOrigin::Custom`]. When writing
/// the pack's files fails, the new instance is deleted again.
pub async fn install_curseforge<I, S, C, A, D, F, P, M, R>(
    request: InstallCurseForgeRequest<'_>,
    deps: &CurseForgeDeps<'_, I, S, C, A, D, F, P, M>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<InstalledCurseForge, InstallCurseForgeError>
where
    I: InstanceRepository,
    S: ServerListStore,
    C: ModpackSource + CurseForgeLookup,
    A: CurseForgeArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let (archive, origin, dir) = match request.pack {
        CurseForgeInput::File(path) => (path.to_path_buf(), InstanceOrigin::Custom, None),
        CurseForgeInput::Catalog { project, file } => {
            let versions = deps.source.versions(project).await?;
            let found = versions
                .iter()
                .find(|version| version.id == file)
                .ok_or_else(|| InstallCurseForgeError::VersionNotFound(file.to_owned()))?;
            let pack_file = found
                .pack_file
                .as_ref()
                .ok_or_else(|| InstallCurseForgeError::NotInstallable(file.to_owned()))?;
            let dir = scratch_dir(deps.files.paths, "download");
            let downloaded =
                download_pack(deps.files.downloader, pack_file, &dir, reporter, cancel).await;
            let archive = match downloaded {
                Ok(archive) => archive,
                Err(error) => {
                    release_dir(deps.files.fs, &dir).await;
                    return Err(error.into());
                }
            };
            let origin = InstanceOrigin::Modpack(ModpackRef {
                source: ModpackSourceKind::CurseForge,
                project: project.to_owned(),
                version: found.id.clone(),
            });
            (archive, origin, Some(dir))
        }
    };
    let result = install_from(&archive, origin, request, deps, reporter, cancel).await;
    if let Some(dir) = dir {
        release_dir(deps.files.fs, &dir).await;
    }
    result
}

async fn install_from<I, S, C, A, D, F, P, M, R>(
    archive: &Path,
    origin: InstanceOrigin,
    request: InstallCurseForgeRequest<'_>,
    deps: &CurseForgeDeps<'_, I, S, C, A, D, F, P, M>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<InstalledCurseForge, InstallCurseForgeError>
where
    I: InstanceRepository,
    S: ServerListStore,
    C: CurseForgeLookup,
    A: CurseForgeArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let contents = read_curseforge(deps.source, deps.archive, archive).await?;
    if cancel.is_cancelled() {
        return Err(InstallCurseForgeError::Cancelled);
    }
    let pack = &contents.pack;
    let name = request
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&pack.name);
    let instance = create_instance(
        NewInstance {
            name,
            game_version: pack.game_version.as_str(),
            loader: pack.loader.clone(),
            origin,
            seed_server: request.seed_server,
        },
        deps.instances,
        deps.servers,
    )
    .await?;
    let files = &deps.files;
    if let Some(claim) = request.claim
        && !claim(&instance.id)
    {
        discard_instance(deps.instances, files.fs, files.paths, &instance.id).await;
        return Err(InstallCurseForgeError::Busy(instance.id));
    }
    let synced = sync_curseforge(
        deps.archive,
        files,
        &instance.id,
        archive,
        &contents,
        &PackManifest::default(),
        reporter,
        cancel,
    )
    .await;
    match synced {
        Ok(synced) => Ok(InstalledCurseForge {
            instance,
            files: synced.summary,
            pending: synced.pending,
        }),
        Err(error) => {
            discard_instance(deps.instances, files.fs, files.paths, &instance.id).await;
            Err(error.into())
        }
    }
}

/// A checked pack and where its files come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurseForgeContents {
    pub pack: CurseForgePack,
    pub files: ResolvedFiles,
}

/// Reads the zip's manifest and resolves its files through the catalog.
pub(crate) async fn read_curseforge<C, A>(
    source: &C,
    archive_port: &A,
    archive: &Path,
) -> Result<CurseForgeContents, InstallCurseForgeError>
where
    C: CurseForgeLookup,
    A: CurseForgeArchive,
{
    let raw = archive_port.read_manifest(archive).await?;
    let pack = CurseForgePack::validate(raw).map_err(PackFileError::from)?;
    let file_ids: Vec<u64> = pack.files.iter().map(|file| file.file_id).collect();
    let project_ids: Vec<u64> = pack
        .files
        .iter()
        .map(|file| file.project_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let api_files = source.files(&file_ids).await?;
    let projects = source.projects(&project_ids).await?;
    let files = resolve_files(&pack.files, &api_files, &projects).map_err(PackFileError::from)?;
    Ok(CurseForgeContents { pack, files })
}

/// Syncs the pack's files, then its overrides, into the instance's game
/// folder, given the previous install's manifest.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn sync_curseforge<A, D, F, P, M, R>(
    archive_port: &A,
    files: &FileSyncDeps<'_, D, F, P, M>,
    instance: &InstanceId,
    archive: &Path,
    contents: &CurseForgeContents,
    previous: &PackManifest,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<SyncedFiles, PackSyncError>
where
    A: CurseForgeArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let staging = scratch_dir(files.paths, "staging");
    let result = async {
        if cancel.is_cancelled() {
            return Err(PackSyncError::Cancelled);
        }
        files.fs.create_dir_all(&staging).await?;
        let staged = archive_port
            .extract_overrides(
                archive,
                &contents.pack.overrides,
                &staging.join("overrides"),
            )
            .await?;
        // Overrides come last: one at the same path as a pack file replaces it.
        let mut entries = contents.files.entries.clone();
        entries.extend(staged.into_iter().map(|file| PackEntry {
            path: file.path,
            source: EntrySource::Staged {
                path: file.staged,
                sha1: file.sha1,
            },
            preserve: false,
        }));
        sync_into(
            files,
            instance,
            &entries,
            &contents.files.manual,
            previous,
            &staging,
            reporter,
            cancel,
        )
        .await
    }
    .await;
    release_dir(files.fs, &staging).await;
    result
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::PathBuf;

    use crate::domain::integrity::{Checksum, HashAlgorithm};
    use crate::domain::launch::RelativePath;
    use crate::domain::modpack::curseforge::{
        CurseForgeFile, CurseForgeProject, MANIFEST_TYPE, RawFileRef, RawLoader, RawManifest,
    };
    use crate::domain::modpack::{ModpackFile, ModpackVersion};
    use crate::testing::{FakeCurseForge, FakeCurseForgeArchive, sha1_hex};

    pub const PROJECT: &str = "396246";
    pub const CDN: &str = "https://edge.forgecdn.net/files/";

    /// The download URL of `name`; fake downloads write the URL as the body.
    pub fn url(name: &str) -> String {
        format!("{CDN}{name}")
    }

    /// A pack file the fake catalog offers, its zip at `url(zip)`.
    pub fn pack_version(file: &str, zip: &str) -> ModpackVersion {
        ModpackVersion {
            id: file.into(),
            name: format!("Pack {file}"),
            version_number: file.into(),
            game_versions: vec![],
            loaders: vec![],
            published: Some(format!("2026-10-{file}")),
            pack_file: Some(ModpackFile {
                url: url(zip),
                filename: zip.into(),
                size: url(zip).len() as u64,
                checksum: Checksum::parse(HashAlgorithm::Sha1, &sha1_hex(url(zip).as_bytes()))
                    .unwrap(),
            }),
        }
    }

    pub fn manifest(version: &str, game: &str, loader: &str, files: &[(u64, u64)]) -> RawManifest {
        RawManifest {
            manifest_type: MANIFEST_TYPE.into(),
            manifest_version: 1,
            name: "Fabulously Optimized".into(),
            version: Some(version.into()),
            game_version: game.into(),
            loaders: vec![RawLoader {
                id: loader.into(),
                primary: true,
            }],
            files: files
                .iter()
                .map(|&(project_id, file_id)| RawFileRef {
                    project_id,
                    file_id,
                    required: true,
                })
                .collect(),
            overrides: Some("overrides".into()),
        }
    }

    /// A downloadable file (`name`) of project `project`, or a manual one.
    pub fn cf_file(id: u64, project: u64, name: &str, downloadable: bool) -> CurseForgeFile {
        CurseForgeFile {
            id,
            project_id: project,
            file_name: name.into(),
            length: Some(url(name).len() as u64),
            sha1: Some(sha1_hex(url(name).as_bytes())),
            download_url: downloadable.then(|| url(name)),
        }
    }

    pub fn cf_project(id: u64, name: &str, class: u64) -> CurseForgeProject {
        CurseForgeProject {
            id,
            name: name.into(),
            class_id: Some(class),
        }
    }

    /// A catalog knowing two projects: Sodium (a mod) and Entity Culling (a
    /// mod whose author forbids third-party downloads), plus a resource pack.
    pub fn catalog(versions: Vec<ModpackVersion>, files: Vec<CurseForgeFile>) -> FakeCurseForge {
        FakeCurseForge {
            versions: [(PROJECT.to_owned(), versions)].into(),
            files,
            projects: vec![
                cf_project(1, "Sodium", 6),
                cf_project(2, "Entity Culling", 6),
                cf_project(3, "Translations", 12),
            ],
            ..FakeCurseForge::default()
        }
    }

    /// A zip archive keyed by its download URL (or its local path).
    /// A pack zip: its key (download URL or local path), manifest and overrides.
    pub type Zip<'a> = (String, RawManifest, Vec<(&'a str, &'a [u8])>);

    pub fn archive(packs: Vec<Zip<'_>>) -> FakeCurseForgeArchive {
        let mut archive = FakeCurseForgeArchive::default();
        for (key, manifest, overrides) in packs {
            archive.manifests.insert(PathBuf::from(&key), manifest);
            archive.overrides.insert(
                PathBuf::from(&key),
                overrides
                    .into_iter()
                    .map(|(path, bytes)| (RelativePath::parse(path).unwrap(), bytes.to_vec()))
                    .collect(),
            );
        }
        archive
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::fixtures::*;
    use super::*;
    use crate::application::modpack_files::fixtures::{FailingDownloader, World, rel};
    use crate::domain::loader::LoaderKind;
    use crate::testing::{
        FakeCurseForge, FakeCurseForgeArchive, FakeDownloader, RecordingReporter, sha1_hex,
    };

    const LOCAL: &str = "/packs/fo.zip";
    const ID: &str = "fabulously-optimized";

    /// Serves the fake archive keyed by the downloaded file's content (its
    /// URL) or, for a local zip, by its path.
    struct ByContent(FakeCurseForgeArchive);

    impl ByContent {
        fn key(&self, archive: &Path) -> PathBuf {
            std::fs::read_to_string(archive)
                .map(PathBuf::from)
                .unwrap_or_else(|_| archive.to_path_buf())
        }
    }

    impl CurseForgeArchive for ByContent {
        async fn read_manifest(
            &self,
            archive: &Path,
        ) -> Result<crate::domain::modpack::curseforge::RawManifest, PackFileError> {
            self.0.read_manifest(&self.key(archive)).await
        }

        async fn extract_overrides(
            &self,
            archive: &Path,
            folder: &str,
            staging: &Path,
        ) -> Result<Vec<crate::domain::modpack::StagedFile>, PackFileError> {
            self.0
                .extract_overrides(&self.key(archive), folder, staging)
                .await
        }
    }

    fn downloader(names: &[&str]) -> FakeDownloader {
        FakeDownloader::with(names.iter().map(|name| (url(name), url(name).into_bytes())))
    }

    fn files() -> Vec<crate::domain::modpack::curseforge::CurseForgeFile> {
        vec![
            cf_file(10, 1, "sodium.jar", true),
            cf_file(20, 2, "entityculling.jar", false),
            cf_file(30, 3, "Translations.zip", true),
        ]
    }

    fn v1_manifest() -> crate::domain::modpack::curseforge::RawManifest {
        manifest(
            "14.1.1",
            "26.2",
            "fabric-0.19.5",
            &[(1, 10), (2, 20), (3, 30)],
        )
    }

    async fn install<C: ModpackSource + CurseForgeLookup>(
        world: &World,
        source: &C,
        archive: &ByContent,
        downloader: &FakeDownloader,
        pack: CurseForgeInput<'_>,
        claim: Option<ClaimInstance<'_>>,
    ) -> Result<InstalledCurseForge, InstallCurseForgeError> {
        let deps = CurseForgeDeps {
            instances: &world.instances,
            servers: &world.servers,
            source,
            archive,
            files: FileSyncDeps {
                downloader,
                fs: &world.fs,
                files: &world.files,
                manifests: &world.manifests,
                paths: &world.paths,
            },
        };
        install_curseforge(
            InstallCurseForgeRequest {
                pack,
                name: None,
                seed_server: None,
                claim,
            },
            &deps,
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
    }

    fn v1_world() -> (World, FakeCurseForge, ByContent, FakeDownloader) {
        let zip = url("fo-14.1.1.zip");
        (
            World::new(vec![]),
            catalog(vec![pack_version("9127912", "fo-14.1.1.zip")], files()),
            ByContent(archive(vec![(
                zip,
                v1_manifest(),
                vec![("config/sodium.json", b"{\"quality\":1}")],
            )])),
            downloader(&["fo-14.1.1.zip", "sodium.jar", "Translations.zip"]),
        )
    }

    #[tokio::test]
    async fn installs_a_catalog_pack_with_its_overrides_and_manual_files() {
        let (world, source, archive, downloader) = v1_world();
        let installed = install(
            &world,
            &source,
            &archive,
            &downloader,
            CurseForgeInput::Catalog {
                project: PROJECT,
                file: "9127912",
            },
            None,
        )
        .await
        .unwrap();

        let instance = &installed.instance;
        assert_eq!(instance.id.as_str(), ID);
        assert_eq!(instance.game_version.as_str(), "26.2");
        assert_eq!(instance.loader.kind(), LoaderKind::Fabric);
        assert_eq!(instance.loader.version().unwrap().as_str(), "0.19.5");
        assert_eq!(
            instance.origin,
            InstanceOrigin::Modpack(ModpackRef {
                source: ModpackSourceKind::CurseForge,
                project: PROJECT.into(),
                version: "9127912".into(),
            })
        );
        // Files are looked up once each, in one batch.
        assert_eq!(source.file_lookups(), [vec![10, 20, 30]]);
        assert_eq!(source.project_lookups(), [vec![1, 2, 3]]);
        assert_eq!(archive.0.folders(), ["overrides"]);

        assert_eq!(
            world.read_game_file(ID, "mods/sodium.jar").unwrap(),
            url("sodium.jar").as_bytes()
        );
        assert_eq!(
            world
                .read_game_file(ID, "resourcepacks/Translations.zip")
                .unwrap(),
            url("Translations.zip").as_bytes()
        );
        assert_eq!(
            world.read_game_file(ID, "config/sodium.json").unwrap(),
            b"{\"quality\":1}"
        );
        assert!(world.read_game_file(ID, "mods/entityculling.jar").is_none());

        assert_eq!(installed.pending.len(), 1);
        assert_eq!(installed.pending[0].path, rel("mods/entityculling.jar"));
        assert_eq!(installed.pending[0].name, "Entity Culling");
        assert_eq!(installed.pending[0].project_id, 2);
        let manifest = world.manifests.load(&instance.id).await.unwrap();
        assert_eq!(manifest.pending, installed.pending);
        assert_eq!(
            manifest.files.get(&rel("mods/sodium.jar")),
            Some(&sha1_hex(url("sodium.jar").as_bytes()))
        );
        // Downloads are verified with the sha1 and size CurseForge declares.
        let requests = downloader.requests();
        let sodium = requests
            .iter()
            .find(|r| r.url == url("sodium.jar"))
            .unwrap();
        assert_eq!(
            sodium.checksum.as_ref().unwrap().value(),
            sha1_hex(url("sodium.jar").as_bytes())
        );
        assert_eq!(sodium.size, Some(url("sodium.jar").len() as u64));
    }

    #[tokio::test]
    async fn imports_a_local_zip_as_a_custom_instance() {
        let (world, source, _, _) = v1_world();
        let archive = ByContent(archive(vec![(LOCAL.into(), v1_manifest(), vec![])]));
        let downloader = downloader(&["sodium.jar", "Translations.zip"]);
        let installed = install(
            &world,
            &source,
            &archive,
            &downloader,
            CurseForgeInput::File(Path::new(LOCAL)),
            None,
        )
        .await
        .unwrap();
        assert_eq!(installed.instance.origin, InstanceOrigin::Custom);
        assert!(
            downloader
                .requests()
                .iter()
                .all(|r| !r.url.ends_with(".zip") || r.url.ends_with("Translations.zip"))
        );
        assert_eq!(installed.pending.len(), 1);
    }

    #[tokio::test]
    async fn refuses_unknown_and_undownloadable_pack_files() {
        let (world, mut source, archive, downloader) = v1_world();
        let missing = install(
            &world,
            &source,
            &archive,
            &downloader,
            CurseForgeInput::Catalog {
                project: PROJECT,
                file: "1",
            },
            None,
        )
        .await;
        assert_eq!(
            missing,
            Err(InstallCurseForgeError::VersionNotFound("1".into()))
        );
        let mut blocked = pack_version("9127912", "fo-14.1.1.zip");
        blocked.pack_file = None;
        source.versions.insert(PROJECT.into(), vec![blocked]);
        let blocked = install(
            &world,
            &source,
            &archive,
            &downloader,
            CurseForgeInput::Catalog {
                project: PROJECT,
                file: "9127912",
            },
            None,
        )
        .await;
        assert_eq!(
            blocked,
            Err(InstallCurseForgeError::NotInstallable("9127912".into()))
        );
        assert!(world.instances.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_pack_that_does_not_resolve_creates_no_instance() {
        let (world, mut source, archive, downloader) = v1_world();
        source.projects[0].class_id = Some(17); // a world, not a mod
        let result = install(
            &world,
            &source,
            &archive,
            &downloader,
            CurseForgeInput::Catalog {
                project: PROJECT,
                file: "9127912",
            },
            None,
        )
        .await;
        assert!(
            matches!(
                result,
                Err(InstallCurseForgeError::Pack(PackSyncError::Files(
                    PackFileError::CurseForge(_)
                )))
            ),
            "{result:?}"
        );
        assert!(world.instances.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_busy_instance_or_a_failed_download_removes_the_new_instance() {
        let (world, source, archive, downloader) = v1_world();
        let refuse = |_: &InstanceId| false;
        let busy = install(
            &world,
            &source,
            &archive,
            &downloader,
            CurseForgeInput::Catalog {
                project: PROJECT,
                file: "9127912",
            },
            Some(&refuse),
        )
        .await;
        assert_eq!(
            busy,
            Err(InstallCurseForgeError::Busy(InstanceId::parse(ID).unwrap()))
        );
        assert!(world.instances.list().await.unwrap().is_empty());

        // The mod downloads fail after the instance exists: nothing stays behind.
        let local = ByContent(super::fixtures::archive(vec![(
            LOCAL.into(),
            v1_manifest(),
            vec![("config/sodium.json", b"{}")],
        )]));
        let deps = CurseForgeDeps {
            instances: &world.instances,
            servers: &world.servers,
            source: &source,
            archive: &local,
            files: FileSyncDeps {
                downloader: &FailingDownloader,
                fs: &world.fs,
                files: &world.files,
                manifests: &world.manifests,
                paths: &world.paths,
            },
        };
        let failed = install_curseforge(
            InstallCurseForgeRequest {
                pack: CurseForgeInput::File(Path::new(LOCAL)),
                name: None,
                seed_server: None,
                claim: None,
            },
            &deps,
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;
        assert!(
            matches!(failed, Err(InstallCurseForgeError::Pack(_))),
            "{failed:?}"
        );
        assert!(world.instances.list().await.unwrap().is_empty());
        assert!(world.read_game_file(ID, "config/sodium.json").is_none());
    }

    #[tokio::test]
    async fn an_unconfigured_catalog_creates_nothing() {
        let (world, _, archive, downloader) = v1_world();
        let source = FakeCurseForge {
            failure: Some(ModpackSourceError::NotConfigured(
                ModpackSourceKind::CurseForge,
            )),
            ..FakeCurseForge::default()
        };
        let result = install(
            &world,
            &source,
            &archive,
            &downloader,
            CurseForgeInput::Catalog {
                project: PROJECT,
                file: "9127912",
            },
            None,
        )
        .await;
        assert_eq!(
            result,
            Err(InstallCurseForgeError::Source(
                ModpackSourceError::NotConfigured(ModpackSourceKind::CurseForge)
            ))
        );
        assert!(downloader.requests().is_empty());
        assert!(world.instances.list().await.unwrap().is_empty());
    }
}
