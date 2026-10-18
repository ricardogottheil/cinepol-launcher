//! Shared by the modpack install and update use cases: downloading a catalog
//! pack and syncing a pack's files into an instance's game folder.
//!
//! Syncing follows [`plan_sync`]: a file is the pack's to replace or delete
//! only while it still matches what the pack installed, so mods the player
//! added and configs the player changed are kept.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::domain::instance::InstanceId;
use crate::domain::launch::{LauncherPaths, RelativePath};
use crate::domain::modpack::mrpack::{MrpackError, MrpackFile, MrpackIndex};
use crate::domain::modpack::sync::{IncomingFile, PackManifest, plan_sync};
use crate::domain::modpack::{
    ModpackFile, MrpackArchive, PackFileError, PackFiles, PackManifestStore, StagedFile,
};
use crate::domain::ports::{
    DownloadError, DownloadRequest, Downloader, FileSystem, FileSystemError, ProgressReporter,
    ProgressStage, StagedReporter,
};

/// The ports a pack sync talks to.
pub struct PackSyncDeps<'a, A, D, F, P, M> {
    pub archive: &'a A,
    pub downloader: &'a D,
    pub fs: &'a F,
    pub files: &'a P,
    pub manifests: &'a M,
    pub paths: &'a LauncherPaths,
}

/// How many pack files were written, already up to date, kept because the
/// player changed them, and removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncSummary {
    pub written: usize,
    pub unchanged: usize,
    pub kept: usize,
    pub removed: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PackSyncError {
    #[error(transparent)]
    Files(#[from] PackFileError),
    #[error(transparent)]
    Download(DownloadError),
    #[error(transparent)]
    Scratch(#[from] FileSystemError),
    #[error("the modpack operation was cancelled")]
    Cancelled,
}

impl From<DownloadError> for PackSyncError {
    fn from(error: DownloadError) -> Self {
        if error == DownloadError::Cancelled {
            Self::Cancelled
        } else {
            Self::Download(error)
        }
    }
}

/// A fresh folder under the launcher's temp folder, unique per run so two
/// installs never share scratch files.
pub(crate) fn scratch_dir(paths: &LauncherPaths, purpose: &str) -> PathBuf {
    paths
        .temp_dir()
        .join("modpacks")
        .join(format!("{purpose}-{}", uuid::Uuid::new_v4().simple()))
}

/// Best-effort removal of a scratch folder; a leftover is only wasted space.
pub(crate) async fn release_dir<F: FileSystem>(fs: &F, dir: &Path) {
    if let Err(error) = fs.remove_dir_all(dir).await {
        tracing::warn!(dir = %dir.display(), %error, "could not remove a modpack scratch folder");
    }
}

/// Downloads a catalog pack into `dir`, verified against its declared hash,
/// and returns the archive path.
pub(crate) async fn download_pack<D: Downloader, R: ProgressReporter>(
    downloader: &D,
    file: &ModpackFile,
    dir: &Path,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<PathBuf, PackSyncError> {
    if cancel.is_cancelled() {
        return Err(PackSyncError::Cancelled);
    }
    // The catalog's file name is not trusted as a path.
    let archive = dir.join("pack.mrpack");
    let request = DownloadRequest {
        url: file.url.clone(),
        destination: archive.clone(),
        checksum: Some(file.checksum.clone()),
        size: Some(file.size),
    };
    downloader
        .download_all(
            vec![request],
            &StagedReporter::new(reporter, ProgressStage::Modpack),
            cancel,
        )
        .await?;
    Ok(archive)
}

/// Where the new content of a path comes from.
enum Source<'p> {
    Index(&'p MrpackFile),
    Override(&'p StagedFile),
}

/// Syncs the pack in `archive` (whose checked index is `index`) into the
/// instance's game folder, given the manifest of the previous install
/// (empty for a fresh install). The new manifest is saved only once every
/// file was written and removed.
pub(crate) async fn sync_pack<A, D, F, P, M, R>(
    deps: &PackSyncDeps<'_, A, D, F, P, M>,
    instance: &InstanceId,
    archive: &Path,
    index: &MrpackIndex,
    previous: &PackManifest,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<SyncSummary, PackSyncError>
where
    A: MrpackArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let staging = scratch_dir(deps.paths, "staging");
    let result = sync_with_staging(
        deps, instance, archive, index, previous, &staging, reporter, cancel,
    )
    .await;
    release_dir(deps.fs, &staging).await;
    result
}

#[allow(clippy::too_many_arguments)]
async fn sync_with_staging<A, D, F, P, M, R>(
    deps: &PackSyncDeps<'_, A, D, F, P, M>,
    instance: &InstanceId,
    archive: &Path,
    index: &MrpackIndex,
    previous: &PackManifest,
    staging: &Path,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<SyncSummary, PackSyncError>
where
    A: MrpackArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    if cancel.is_cancelled() {
        return Err(PackSyncError::Cancelled);
    }
    deps.fs.create_dir_all(staging).await?;
    let overrides_dir = staging.join("overrides");
    let downloads_dir = staging.join("downloads");
    let staged = deps
        .archive
        .extract_overrides(archive, &overrides_dir)
        .await?;

    // Overrides come last: one at the same path as an index file replaces it.
    let mut sources: BTreeMap<&RelativePath, Source<'_>> = BTreeMap::new();
    let mut incoming = Vec::new();
    for file in index.client_files() {
        incoming.push(IncomingFile {
            path: file.path.clone(),
            sha1: file.sha1.value().to_owned(),
        });
        sources.insert(&file.path, Source::Index(file));
    }
    for file in &staged {
        incoming.push(IncomingFile {
            path: file.path.clone(),
            sha1: file.sha1.clone(),
        });
        sources.insert(&file.path, Source::Override(file));
    }

    let game_dir = deps.paths.game_dir(instance);
    let mut on_disk = BTreeMap::new();
    for path in previous.files.keys().chain(sources.keys().copied()) {
        if !on_disk.contains_key(path) {
            let sha1 = deps.files.sha1(&path.under(&game_dir)).await?;
            on_disk.insert(path.clone(), sha1);
        }
    }
    let plan = plan_sync(previous, &incoming, &on_disk);
    if cancel.is_cancelled() {
        return Err(PackSyncError::Cancelled);
    }

    // Everything is downloaded into staging first: an interrupted download
    // leaves the game folder as it was, and only local moves follow.
    let mut downloads = Vec::new();
    let mut placements: Vec<(PathBuf, &RelativePath)> = Vec::new();
    for path in &plan.write {
        match sources.get(path) {
            Some(Source::Index(file)) => {
                downloads.push(download_request(file, &downloads_dir)?);
                placements.push((path.under(&downloads_dir), path));
            }
            Some(Source::Override(file)) => placements.push((file.staged.clone(), path)),
            // `plan_sync` only writes incoming paths, which all have a source.
            None => {}
        }
    }
    deps.downloader
        .download_all(
            downloads,
            &StagedReporter::new(reporter, ProgressStage::Modpack),
            cancel,
        )
        .await?;
    if cancel.is_cancelled() {
        return Err(PackSyncError::Cancelled);
    }
    for (from, path) in placements {
        deps.files.place(&from, &path.under(&game_dir)).await?;
    }
    for path in &plan.remove {
        deps.files.remove_file(&path.under(&game_dir)).await?;
    }
    // Record what was actually written: downloads are verified against sha512
    // only, and a wrong declared sha1 would make an intact file look changed.
    let mut manifest = plan.manifest;
    for path in &plan.write {
        if let Some(sha1) = deps.files.sha1(&path.under(&game_dir)).await? {
            manifest.files.insert(path.clone(), sha1);
        }
    }
    deps.manifests.save(instance, &manifest).await?;
    Ok(SyncSummary {
        written: plan.write.len(),
        unchanged: plan.unchanged.len(),
        kept: plan.kept.len(),
        removed: plan.remove.len(),
    })
}

/// Index files are verified against their sha512 while downloading.
fn download_request(file: &MrpackFile, game_dir: &Path) -> Result<DownloadRequest, PackFileError> {
    let url = file
        .downloads
        .first()
        .ok_or_else(|| MrpackError::NoDownload(file.path.to_string()))?;
    Ok(DownloadRequest {
        url: url.clone(),
        destination: file.path.under(game_dir),
        checksum: Some(file.sha512.clone()),
        size: Some(file.size),
    })
}

/// Shared test setup for the modpack use cases.
#[cfg(test)]
pub(crate) mod fixtures {
    use std::collections::{BTreeMap, HashMap};
    use std::path::{Path, PathBuf};

    use super::PackSyncDeps;
    use crate::domain::integrity::{Checksum, HashAlgorithm};
    use crate::domain::launch::{LauncherPaths, RelativePath};
    use crate::domain::modpack::mrpack::{
        MrpackIndex, RawMrpackEnv, RawMrpackFile, RawMrpackIndex,
    };
    use crate::domain::modpack::{
        ModpackFile, ModpackPage, ModpackSourceKind, ModpackVersion, MrpackArchive, PackFileError,
        StagedFile,
    };
    use crate::testing::{
        FakeDownloader, FakeFileSystem, FakeModpackSource, FakeMrpackArchive, FakePackFiles,
        InMemoryInstanceRepository, InMemoryPackManifestStore, InMemoryServerListStore, sha1_hex,
        sha512_hex,
    };

    /// The download URL of a pack file; the fake downloader serves the URL
    /// itself as the file's content, so every version's file differs.
    pub fn url(version: &str, path: &str) -> String {
        format!("https://cdn.modrinth.com/data/fo/versions/{version}/{path}")
    }

    /// An index entry served from [`url`], with `client` as its client env.
    pub fn entry(version: &str, path: &str, client: &str) -> RawMrpackFile {
        let url = url(version, path);
        RawMrpackFile {
            path: path.into(),
            hashes: BTreeMap::from([
                ("sha1".into(), sha1_hex(url.as_bytes())),
                ("sha512".into(), sha512_hex(url.as_bytes())),
            ]),
            env: Some(RawMrpackEnv {
                client: Some(client.into()),
                server: Some("required".into()),
            }),
            downloads: vec![url.clone()],
            file_size: url.len() as u64,
        }
    }

    /// A Fabulously Optimized index for `game` and Fabric `fabric`.
    pub fn index(
        version: &str,
        game: &str,
        fabric: &str,
        files: Vec<RawMrpackFile>,
    ) -> MrpackIndex {
        MrpackIndex::validate(RawMrpackIndex {
            format_version: 1,
            game: "minecraft".into(),
            version_id: version.into(),
            name: "Fabulously Optimized".into(),
            summary: None,
            files,
            dependencies: BTreeMap::from([
                ("minecraft".into(), game.into()),
                ("fabric-loader".into(), fabric.into()),
            ]),
        })
        .unwrap()
    }

    pub fn rel(raw: &str) -> RelativePath {
        RelativePath::parse(raw).unwrap()
    }

    /// The URL of a catalog version's `.mrpack`.
    pub fn pack_url(version: &str) -> String {
        format!("https://cdn.modrinth.com/data/fo/versions/{version}/fo.mrpack")
    }

    pub fn pack_file(version: &str) -> ModpackFile {
        let url = pack_url(version);
        ModpackFile {
            filename: "fo.mrpack".into(),
            size: url.len() as u64,
            checksum: Checksum::parse(HashAlgorithm::Sha512, &sha512_hex(url.as_bytes())).unwrap(),
            url,
        }
    }

    pub fn catalog_version(id: &str, pack: bool) -> ModpackVersion {
        ModpackVersion {
            id: id.into(),
            name: format!("Fabulously Optimized {id}"),
            version_number: id.into(),
            game_versions: vec![],
            loaders: vec![],
            published: None,
            pack_file: pack.then(|| pack_file(id)),
        }
    }

    /// The fake archive, which also resolves a downloaded catalog pack: the
    /// fake downloader writes the pack's URL as its content, and packs served
    /// by the catalog are registered under that URL.
    #[derive(Debug, Default)]
    pub struct TestArchive(pub FakeMrpackArchive);

    impl TestArchive {
        fn key(&self, archive: &Path) -> PathBuf {
            if self.0.indexes.contains_key(archive) {
                return archive.to_path_buf();
            }
            std::fs::read_to_string(archive)
                .map(PathBuf::from)
                .unwrap_or_else(|_| archive.to_path_buf())
        }
    }

    impl MrpackArchive for TestArchive {
        async fn read_index(&self, archive: &Path) -> Result<MrpackIndex, PackFileError> {
            self.0.read_index(&self.key(archive)).await
        }

        async fn extract_overrides(
            &self,
            archive: &Path,
            staging: &Path,
        ) -> Result<Vec<StagedFile>, PackFileError> {
            self.0.extract_overrides(&self.key(archive), staging).await
        }

        async fn write(
            &self,
            archive: &Path,
            index: &MrpackIndex,
            overrides: &[(RelativePath, PathBuf)],
        ) -> Result<(), PackFileError> {
            self.0.write(archive, index, overrides).await
        }
    }

    /// A pack: its archive key (a local path or a catalog pack URL), index and overrides.
    pub type Pack = (String, MrpackIndex, Vec<(&'static str, &'static [u8])>);

    pub struct World {
        /// Keeps the data folder alive for the test.
        pub _dir: tempfile::TempDir,
        pub paths: LauncherPaths,
        pub archive: TestArchive,
        pub downloader: FakeDownloader,
        pub fs: FakeFileSystem,
        pub files: FakePackFiles,
        pub manifests: InMemoryPackManifestStore,
        pub instances: InMemoryInstanceRepository,
        pub servers: InMemoryServerListStore,
        pub source: FakeModpackSource,
    }

    impl World {
        pub fn new(packs: Vec<Pack>) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let paths = LauncherPaths::new(dir.path());
            let mut archive = FakeMrpackArchive::default();
            let mut bodies = Vec::new();
            for (key, index, overrides) in packs {
                for file in &index.files {
                    let url = file.downloads[0].clone();
                    bodies.push((url.clone(), url.into_bytes()));
                }
                if key.starts_with("https://") {
                    bodies.push((key.clone(), key.clone().into_bytes()));
                }
                archive.indexes.insert(PathBuf::from(&key), index);
                archive.overrides.insert(
                    PathBuf::from(&key),
                    overrides
                        .into_iter()
                        .map(|(path, bytes)| (rel(path), bytes.to_vec()))
                        .collect(),
                );
            }
            Self {
                _dir: dir,
                paths,
                archive: TestArchive(archive),
                downloader: FakeDownloader::with(bodies),
                fs: FakeFileSystem::default(),
                files: FakePackFiles::default(),
                manifests: InMemoryPackManifestStore::default(),
                instances: InMemoryInstanceRepository::default(),
                servers: InMemoryServerListStore::default(),
                source: FakeModpackSource {
                    kind: ModpackSourceKind::Modrinth,
                    page: ModpackPage {
                        items: vec![],
                        total: None,
                    },
                    versions: HashMap::new(),
                    known: HashMap::new(),
                },
            }
        }

        pub fn pack_deps<'a, D>(
            &'a self,
            downloader: &'a D,
        ) -> PackSyncDeps<
            'a,
            TestArchive,
            D,
            FakeFileSystem,
            FakePackFiles,
            InMemoryPackManifestStore,
        > {
            PackSyncDeps {
                archive: &self.archive,
                downloader,
                fs: &self.fs,
                files: &self.files,
                manifests: &self.manifests,
                paths: &self.paths,
            }
        }

        /// A file in the game folder of instance `id`.
        pub fn game_file(&self, id: &str, path: &str) -> PathBuf {
            let id = crate::domain::instance::InstanceId::parse(id).unwrap();
            rel(path).under(&self.paths.game_dir(&id))
        }

        pub fn write_game_file(&self, id: &str, path: &str, bytes: &[u8]) {
            let target = self.game_file(id, path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, bytes).unwrap();
        }

        pub fn read_game_file(&self, id: &str, path: &str) -> Option<Vec<u8>> {
            std::fs::read(self.game_file(id, path)).ok()
        }
    }

    /// A downloader that always fails, to exercise failed syncs.
    #[derive(Debug, Default)]
    pub struct FailingDownloader;

    impl crate::domain::ports::Downloader for FailingDownloader {
        async fn download_all<R: crate::domain::ports::ProgressReporter>(
            &self,
            requests: Vec<crate::domain::ports::DownloadRequest>,
            _reporter: &R,
            _cancel: &tokio_util::sync::CancellationToken,
        ) -> Result<crate::domain::ports::DownloadSummary, crate::domain::ports::DownloadError>
        {
            if requests.is_empty() {
                return Ok(Default::default());
            }
            Err(crate::domain::ports::DownloadError::Http {
                url: requests[0].url.clone(),
                reason: "connection reset".into(),
            })
        }
    }
}
