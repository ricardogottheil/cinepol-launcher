//! Shared by the modpack install and update use cases: downloading a catalog
//! pack and syncing a pack's files into an instance's game folder.
//!
//! Syncing follows [`plan_sync`]: a file is the pack's to replace or delete
//! only while it still matches what the pack installed, so mods the player
//! added and configs the player changed are kept. Every pack format turns its
//! files into [`PackEntry`]s and goes through [`sync_files`]: downloads and
//! local copies land in a staging folder and are verified there, and only
//! then are files moved into the game folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::domain::instance::InstanceId;
use crate::domain::integrity::Checksum;
use crate::domain::launch::{LauncherPaths, RelativePath};
use crate::domain::modpack::mrpack::MrpackIndex;
use crate::domain::modpack::sync::{
    EntrySource, IncomingFile, ManualFile, PackEntry, PackManifest, plan_sync,
};
use crate::domain::modpack::{
    ModpackFile, MrpackArchive, PackFileError, PackFiles, PackManifestStore,
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

impl<'a, A, D, F, P, M> PackSyncDeps<'a, A, D, F, P, M> {
    /// The ports the shared file sync needs (all but the archive).
    pub fn file_sync(&self) -> FileSyncDeps<'a, D, F, P, M> {
        FileSyncDeps {
            downloader: self.downloader,
            fs: self.fs,
            files: self.files,
            manifests: self.manifests,
            paths: self.paths,
        }
    }
}

/// The ports the shared file sync talks to.
pub struct FileSyncDeps<'a, D, F, P, M> {
    pub downloader: &'a D,
    pub fs: &'a F,
    pub files: &'a P,
    pub manifests: &'a M,
    pub paths: &'a LauncherPaths,
}

/// What a sync did, and the files still waiting for a manual download.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncedFiles {
    pub summary: SyncSummary,
    pub pending: Vec<ManualFile>,
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
    let staged = deps
        .archive
        .extract_overrides(archive, &staging.join("overrides"))
        .await?;

    // Index files are verified against their sha512 while downloading.
    // Overrides come last: one at the same path as an index file replaces it.
    let mut entries: Vec<PackEntry> = index
        .client_files()
        .map(|file| PackEntry {
            path: file.path.clone(),
            source: EntrySource::Download {
                urls: file.downloads.clone(),
                checksum: file.sha512.clone(),
                size: Some(file.size),
            },
            preserve: false,
        })
        .collect();
    entries.extend(staged.into_iter().map(|file| PackEntry {
        path: file.path,
        source: EntrySource::Staged {
            path: file.staged,
            sha1: file.sha1,
        },
        preserve: false,
    }));
    let synced = sync_into(
        &deps.file_sync(),
        instance,
        &entries,
        &[],
        previous,
        staging,
        reporter,
        cancel,
    )
    .await?;
    Ok(synced.summary)
}

/// Syncs `entries` into the instance's game folder, given the manifest of the
/// previous install (empty for a fresh install). `manual` files the player
/// still has to download are recorded as pending, except those already in
/// place with the right hash. The new manifest is saved only once every file
/// was written and removed.
#[allow(clippy::too_many_arguments)]
pub async fn sync_files<D, F, P, M, R>(
    deps: &FileSyncDeps<'_, D, F, P, M>,
    instance: &InstanceId,
    entries: &[PackEntry],
    manual: &[ManualFile],
    previous: &PackManifest,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<SyncedFiles, PackSyncError>
where
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let staging = scratch_dir(deps.paths, "staging");
    let result = async {
        if cancel.is_cancelled() {
            return Err(PackSyncError::Cancelled);
        }
        deps.fs.create_dir_all(&staging).await?;
        sync_into(
            deps, instance, entries, manual, previous, &staging, reporter, cancel,
        )
        .await
    }
    .await;
    release_dir(deps.fs, &staging).await;
    result
}

/// Stands in for the sha1 of a file that is not on disk yet: it never matches
/// a real sha1, so the planner writes the file unless the player changed it.
const UNWRITTEN: &str = "unwritten";

/// The shared pipeline; `staging` exists and is released by the caller.
#[allow(clippy::too_many_arguments)]
async fn sync_into<D, F, P, M, R>(
    deps: &FileSyncDeps<'_, D, F, P, M>,
    instance: &InstanceId,
    entries: &[PackEntry],
    manual: &[ManualFile],
    previous: &PackManifest,
    staging: &Path,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<SyncedFiles, PackSyncError>
where
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let game_dir = deps.paths.game_dir(instance);
    // A later entry for the same path wins.
    let mut sources: BTreeMap<&RelativePath, &PackEntry> = BTreeMap::new();
    for entry in entries {
        sources.insert(&entry.path, entry);
    }
    let manual: Vec<&ManualFile> = manual
        .iter()
        .filter(|file| !sources.contains_key(&file.path))
        .collect();

    let mut on_disk = BTreeMap::new();
    let wanted = sources
        .keys()
        .copied()
        .chain(manual.iter().map(|f| &f.path));
    for path in previous.files.keys().chain(wanted) {
        if !on_disk.contains_key(path) {
            let sha1 = deps.files.sha1(&path.under(&game_dir)).await?;
            on_disk.insert(path.clone(), sha1);
        }
    }
    let disk = |path: &RelativePath| on_disk.get(path).cloned().flatten();

    let mut incoming = Vec::new();
    for (path, entry) in &sources {
        let target = path.under(&game_dir);
        let recorded = previous.files.get(*path).map(String::as_str);
        let sha1 = incoming_sha1(deps.files, entry, &target, disk(path), recorded).await?;
        incoming.push(IncomingFile {
            path: (*path).clone(),
            sha1,
        });
    }
    // A manual file already in place with the right hash is installed.
    let mut pending = Vec::new();
    for file in manual {
        match disk(&file.path) {
            Some(sha1)
                if has_checksum(deps.files, &file.path.under(&game_dir), &file.checksum)
                    .await? =>
            {
                incoming.push(IncomingFile {
                    path: file.path.clone(),
                    sha1,
                })
            }
            _ => pending.push(file.clone()),
        }
    }

    let plan = plan_sync(previous, &incoming, &on_disk);
    if cancel.is_cancelled() {
        return Err(PackSyncError::Cancelled);
    }

    // Everything is downloaded or copied into staging first and verified
    // there: a failure leaves the game folder as it was, and only local moves
    // follow.
    let downloads_dir = staging.join("downloads");
    let local_dir = staging.join("local");
    let mut downloads = Vec::new();
    let mut placements: Vec<(PathBuf, &RelativePath)> = Vec::new();
    for path in &plan.write {
        // `plan_sync` only writes incoming paths; matched manual files are
        // already current, so every written path has a source.
        let Some(entry) = sources.get(path) else {
            continue;
        };
        match &entry.source {
            EntrySource::Download {
                urls,
                checksum,
                size,
            } => {
                let url = urls
                    .first()
                    .ok_or_else(|| PackFileError::Source(format!("{path} has no download URL")))?;
                let staged = path.under(&downloads_dir);
                downloads.push(DownloadRequest {
                    url: url.clone(),
                    destination: staged.clone(),
                    checksum: Some(checksum.clone()),
                    size: *size,
                });
                placements.push((staged, path));
            }
            EntrySource::Local {
                path: from,
                checksum,
            } => {
                let staged = path.under(&local_dir);
                deps.files.place(from, &staged).await?;
                if !has_checksum(deps.files, &staged, checksum).await? {
                    return Err(PackFileError::Source(format!(
                        "{path} does not match its declared hash"
                    ))
                    .into());
                }
                placements.push((staged, path));
            }
            EntrySource::Staged { path: from, .. } => placements.push((from.clone(), path)),
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
    // Record what was actually written: a declared sha1 may be wrong, and
    // other sources declare none.
    let mut manifest = plan.manifest;
    for path in &plan.write {
        match deps.files.sha1(&path.under(&game_dir)).await? {
            Some(sha1) => manifest.files.insert(path.clone(), sha1),
            None => manifest.files.remove(path),
        };
    }
    manifest.pending = pending.clone();
    deps.manifests.save(instance, &manifest).await?;
    Ok(SyncedFiles {
        summary: SyncSummary {
            written: plan.write.len(),
            unchanged: plan.unchanged.len(),
            kept: plan.kept.len(),
            removed: plan.remove.len(),
        },
        pending,
    })
}

/// The sha1 the planner compares the file on disk with: the disk's own when
/// it already holds the entry's content (or, for a preserved entry, the
/// intact copy the pack wrote before), so the plan leaves it alone.
async fn incoming_sha1<P: PackFiles>(
    files: &P,
    entry: &PackEntry,
    target: &Path,
    disk: Option<String>,
    recorded: Option<&str>,
) -> Result<String, PackFileError> {
    let declared = match &entry.source {
        EntrySource::Staged { sha1, .. } => sha1.as_str(),
        EntrySource::Download { .. } | EntrySource::Local { .. } => UNWRITTEN,
    };
    let Some(disk) = disk else {
        return Ok(declared.to_owned());
    };
    let current = match &entry.source {
        EntrySource::Staged { sha1, .. } => sha1.eq_ignore_ascii_case(&disk),
        EntrySource::Download { checksum, .. } | EntrySource::Local { checksum, .. } => {
            has_checksum(files, target, checksum).await?
        }
    };
    let intact = recorded.is_some_and(|old| old.eq_ignore_ascii_case(&disk));
    if current || (entry.preserve && intact) {
        Ok(disk)
    } else {
        Ok(declared.to_owned())
    }
}

/// Whether the file exists and matches `checksum`.
pub(crate) async fn has_checksum<P: PackFiles>(
    files: &P,
    path: &Path,
    checksum: &Checksum,
) -> Result<bool, PackFileError> {
    Ok(files
        .digest(path, checksum.algorithm())
        .await?
        .is_some_and(|digest| checksum.matches(&digest)))
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

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::domain::integrity::{Checksum, HashAlgorithm, curseforge_fingerprint};
    use crate::testing::{FakeDownloader, RecordingReporter, sha1_hex, sha512_hex};

    const ID: &str = "pack";

    fn id() -> InstanceId {
        InstanceId::parse(ID).unwrap()
    }

    fn sha512(bytes: &[u8]) -> Checksum {
        Checksum::parse(HashAlgorithm::Sha512, &sha512_hex(bytes)).unwrap()
    }

    fn cdn(path: &str) -> String {
        format!("https://cdn.example.com/{path}")
    }

    /// A file downloaded from [`cdn`] whose content is `bytes`.
    fn remote(path: &str, bytes: &[u8]) -> PackEntry {
        PackEntry {
            path: rel(path),
            source: EntrySource::Download {
                urls: vec![cdn(path)],
                checksum: sha512(bytes),
                size: Some(bytes.len() as u64),
            },
            preserve: false,
        }
    }

    fn serving(files: &[(&str, &[u8])]) -> FakeDownloader {
        FakeDownloader::with(
            files
                .iter()
                .map(|(path, bytes)| (cdn(path), bytes.to_vec())),
        )
    }

    fn manual(path: &str, bytes: &[u8]) -> ManualFile {
        ManualFile {
            path: rel(path),
            name: "Just Enough Items".into(),
            filename: path.rsplit('/').next().unwrap().into(),
            checksum: Checksum::parse(
                HashAlgorithm::Murmur2,
                &curseforge_fingerprint(bytes).to_string(),
            )
            .unwrap(),
            project_id: 238_222,
            file_id: 1,
        }
    }

    async fn sync<D: Downloader>(
        world: &World,
        downloader: &D,
        entries: &[PackEntry],
        manual: &[ManualFile],
        cancel: &CancellationToken,
    ) -> Result<SyncedFiles, PackSyncError> {
        let deps = FileSyncDeps {
            downloader,
            fs: &world.fs,
            files: &world.files,
            manifests: &world.manifests,
            paths: &world.paths,
        };
        let previous = world.manifests.load(&id()).await.unwrap();
        sync_files(
            &deps,
            &id(),
            entries,
            manual,
            &previous,
            &RecordingReporter::default(),
            cancel,
        )
        .await
    }

    async fn run<D: Downloader>(
        world: &World,
        downloader: &D,
        entries: &[PackEntry],
        manual: &[ManualFile],
    ) -> Result<SyncedFiles, PackSyncError> {
        sync(
            world,
            downloader,
            entries,
            manual,
            &CancellationToken::new(),
        )
        .await
    }

    fn recorded(world: &World) -> Vec<(String, String)> {
        let manifests = world.manifests.manifests.lock().unwrap();
        manifests[&id()]
            .files
            .iter()
            .map(|(path, sha1)| (path.to_string(), sha1.clone()))
            .collect()
    }

    #[tokio::test]
    async fn downloads_into_staging_then_places_and_records_the_written_files() {
        let world = World::new(vec![]);
        let downloader = serving(&[("mods/a.jar", b"a"), ("config/b.toml", b"b")]);
        let entries = [remote("mods/a.jar", b"a"), remote("config/b.toml", b"b")];

        let synced = run(&world, &downloader, &entries, &[]).await.unwrap();

        assert_eq!(
            synced.summary,
            SyncSummary {
                written: 2,
                ..Default::default()
            }
        );
        let requests = downloader.requests();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            assert!(request.destination.starts_with(world.paths.temp_dir()));
        }
        let a = requests
            .iter()
            .find(|r| r.url == cdn("mods/a.jar"))
            .unwrap();
        assert_eq!(a.checksum, Some(sha512(b"a")));
        assert_eq!(a.size, Some(1));
        assert_eq!(world.read_game_file(ID, "mods/a.jar").unwrap(), b"a");
        assert_eq!(world.read_game_file(ID, "config/b.toml").unwrap(), b"b");
        assert_eq!(
            recorded(&world),
            [
                ("config/b.toml".into(), sha1_hex(b"b")),
                ("mods/a.jar".into(), sha1_hex(b"a")),
            ]
        );

        // A second sync finds both files current and downloads nothing.
        let again = run(&world, &downloader, &entries, &[]).await.unwrap();
        assert_eq!(again.summary.unchanged, 2);
        assert_eq!(again.summary.written, 0);
        assert_eq!(downloader.requests().len(), 2);
    }

    #[tokio::test]
    async fn copies_local_files_after_verifying_them_and_refuses_a_mismatch() {
        let world = World::new(vec![]);
        let pack = world.paths.temp_dir().join("local-pack");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join("good.toml"), b"good").unwrap();
        std::fs::write(pack.join("bad.toml"), b"tampered").unwrap();
        let local = |path: &str, file: &str, declared: &[u8]| PackEntry {
            path: rel(path),
            source: EntrySource::Local {
                path: pack.join(file),
                checksum: Checksum::parse(HashAlgorithm::Sha1, &sha1_hex(declared)).unwrap(),
            },
            preserve: false,
        };

        let good = [local("config/good.toml", "good.toml", b"good")];
        run(&world, &FakeDownloader::default(), &good, &[])
            .await
            .unwrap();
        assert_eq!(
            world.read_game_file(ID, "config/good.toml").unwrap(),
            b"good"
        );
        assert!(pack.join("good.toml").exists(), "the source is copied");

        let world = World::new(vec![]);
        let bad = [local("config/bad.toml", "bad.toml", b"original")];
        let result = run(&world, &FakeDownloader::default(), &bad, &[]).await;
        assert!(
            matches!(result, Err(PackSyncError::Files(PackFileError::Source(_)))),
            "{result:?}"
        );
        assert!(world.read_game_file(ID, "config/bad.toml").is_none());
        assert!(world.manifests.manifests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn preserved_files_are_written_once_and_never_replaced() {
        let world = World::new(vec![]);
        let preserved = |bytes: &[u8]| PackEntry {
            preserve: true,
            ..remote("config/options.toml", bytes)
        };
        let v1 = serving(&[("config/options.toml", b"v1")]);
        run(&world, &v1, &[preserved(b"v1")], &[]).await.unwrap();
        assert_eq!(
            world.read_game_file(ID, "config/options.toml").unwrap(),
            b"v1"
        );

        // The pack ships a new version: the intact file stays as it is.
        let v2 = serving(&[("config/options.toml", b"v2")]);
        let synced = run(&world, &v2, &[preserved(b"v2")], &[]).await.unwrap();
        assert!(v2.requests().is_empty());
        assert_eq!(synced.summary.written, 0);
        assert_eq!(
            world.read_game_file(ID, "config/options.toml").unwrap(),
            b"v1"
        );

        // Nor is a copy the player changed.
        world.write_game_file(ID, "config/options.toml", b"player");
        let synced = run(&world, &v2, &[preserved(b"v2")], &[]).await.unwrap();
        assert_eq!(synced.summary.kept, 1);
        assert_eq!(
            world.read_game_file(ID, "config/options.toml").unwrap(),
            b"player"
        );
    }

    #[tokio::test]
    async fn manual_files_stay_pending_unless_already_on_disk() {
        let world = World::new(vec![]);
        world.write_game_file(ID, "mods/have.jar", b"have it");
        let have = manual("mods/have.jar", b"have it");
        let missing = manual("mods/jei.jar", b"jei");

        let synced = run(
            &world,
            &FakeDownloader::default(),
            &[],
            &[have.clone(), missing.clone()],
        )
        .await
        .unwrap();

        assert_eq!(synced.pending, std::slice::from_ref(&missing));
        let manifests = world.manifests.manifests.lock().unwrap();
        assert_eq!(manifests[&id()].pending, [missing]);
        assert_eq!(
            manifests[&id()].files.get(&rel("mods/have.jar")),
            Some(&sha1_hex(b"have it"))
        );
    }

    #[tokio::test]
    async fn a_failed_sync_saves_no_manifest_and_writes_nothing() {
        let world = World::new(vec![]);
        let result = run(
            &world,
            &FailingDownloader,
            &[remote("mods/a.jar", b"a")],
            &[manual("mods/jei.jar", b"jei")],
        )
        .await;
        assert!(matches!(result, Err(PackSyncError::Download(_))));
        assert!(world.manifests.manifests.lock().unwrap().is_empty());
        assert!(world.read_game_file(ID, "mods/a.jar").is_none());
    }

    #[tokio::test]
    async fn cancellation_stops_before_downloading() {
        let world = World::new(vec![]);
        let downloader = serving(&[("mods/a.jar", b"a")]);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = sync(
            &world,
            &downloader,
            &[remote("mods/a.jar", b"a")],
            &[],
            &cancel,
        )
        .await;
        assert_eq!(result, Err(PackSyncError::Cancelled));
        assert!(downloader.requests().is_empty());
        assert!(world.manifests.manifests.lock().unwrap().is_empty());
    }
}
