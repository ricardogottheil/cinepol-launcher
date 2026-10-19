//! Installs a packwiz pack, from a `pack.toml` URL or a local path, into a
//! new instance.
//!
//! `pack.toml` points at the index, verified with the hash `pack.toml`
//! declares; every metafile is verified with the hash the index declares, and
//! every file with the hash its metafile or index entry declares. Files in
//! CurseForge mode are not downloaded: they are returned (and stored in the
//! manifest) as manual downloads. Like `install_modpack`, this does not
//! install Minecraft or the loader.

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::create_instance::{CreateInstanceError, NewInstance, create_instance};
use crate::application::install_modpack::{ClaimInstance, discard_instance};
use crate::application::modpack_files::{FileSyncDeps, PackSyncError, SyncSummary, sync_files};
use crate::domain::instance::{Instance, InstanceId, InstanceOrigin, InstanceRepository};
use crate::domain::launch::RelativePath;
use crate::domain::modpack::packwiz::{
    Metafile, MetafileSource, PackwizError, PackwizIndex, PackwizPack,
};
use crate::domain::modpack::sync::{EntrySource, ManualFile, PackEntry, PackManifest};
use crate::domain::modpack::{
    ModpackRef, ModpackSourceKind, PackFileError, PackFileLocation, PackFiles, PackLocation,
    PackManifestStore, PackwizSource,
};
use crate::domain::ports::{Downloader, FileSystem, ProgressReporter};
use crate::domain::server_list::{ServerEntry, ServerListStore};

/// The origin version of a pack whose `pack.toml` declares none.
pub const UNVERSIONED: &str = "unversioned";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstallPackwizError {
    #[error(transparent)]
    Create(#[from] CreateInstanceError),
    #[error(transparent)]
    Pack(PackSyncError),
    #[error("the modpack installation was cancelled")]
    Cancelled,
    #[error("instance {0} is busy")]
    Busy(InstanceId),
}

impl From<PackSyncError> for InstallPackwizError {
    fn from(error: PackSyncError) -> Self {
        match error {
            PackSyncError::Cancelled => Self::Cancelled,
            other => Self::Pack(other),
        }
    }
}

pub struct InstallPackwizRequest<'a> {
    pub pack: &'a PackLocation,
    /// The instance name; the pack's name when `None` or blank.
    pub name: Option<&'a str>,
    /// Put first in the new instance's `servers.dat` (the CINEPOL server).
    pub seed_server: Option<ServerEntry>,
    /// Called once the new instance exists and before any file is written.
    /// Returning `false` aborts with [`InstallPackwizError::Busy`].
    pub claim: Option<ClaimInstance<'a>>,
}

/// Everything the packwiz installer talks to.
pub struct InstallPackwizDeps<'a, I, S, W, D, F, P, M> {
    pub instances: &'a I,
    pub servers: &'a S,
    pub source: &'a W,
    pub files: FileSyncDeps<'a, D, F, P, M>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackwiz {
    pub instance: Instance,
    pub files: SyncSummary,
    /// Files the player must download by hand to complete the pack.
    pub pending: Vec<ManualFile>,
}

/// The new instance gets a [`InstanceOrigin::Modpack`] origin pointing at the
/// pack's location, so it can be updated later. When writing the pack's
/// files fails, the new instance is deleted again (metadata and folder).
pub async fn install_packwiz<I, S, W, D, F, P, M, R>(
    request: InstallPackwizRequest<'_>,
    deps: &InstallPackwizDeps<'_, I, S, W, D, F, P, M>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<InstalledPackwiz, InstallPackwizError>
where
    I: InstanceRepository,
    S: ServerListStore,
    W: PackwizSource,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let contents = read_packwiz(deps.source, request.pack, cancel).await?;
    if cancel.is_cancelled() {
        return Err(InstallPackwizError::Cancelled);
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
            origin: packwiz_origin(request.pack, pack),
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
        return Err(InstallPackwizError::Busy(instance.id));
    }
    let synced = sync_files(
        files,
        &instance.id,
        &contents.entries,
        &contents.manual,
        &PackManifest::default(),
        reporter,
        cancel,
    )
    .await;
    match synced {
        Ok(synced) => Ok(InstalledPackwiz {
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

/// The origin of an instance installed from `pack` at `location`.
pub(crate) fn packwiz_origin(location: &PackLocation, pack: &PackwizPack) -> InstanceOrigin {
    InstanceOrigin::Modpack(ModpackRef {
        source: ModpackSourceKind::Packwiz,
        project: location.as_string(),
        version: pack
            .version
            .clone()
            .unwrap_or_else(|| UNVERSIONED.to_owned()),
    })
}

/// A checked pack and the files it wants on the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackwizContents {
    pub pack: PackwizPack,
    pub entries: Vec<PackEntry>,
    pub manual: Vec<ManualFile>,
}

/// Reads `pack.toml`, its verified index and every verified metafile.
/// Server-only metafiles are skipped; CurseForge ones become manual files.
pub(crate) async fn read_packwiz<W: PackwizSource>(
    source: &W,
    location: &PackLocation,
    cancel: &CancellationToken,
) -> Result<PackwizContents, PackSyncError> {
    let pack = PackwizPack::validate(source.read_pack(location).await?).map_err(files_error)?;
    let raw_index = source
        .read_index(location, &pack.index, &pack.index_checksum)
        .await?;
    let index = PackwizIndex::validate(raw_index).map_err(files_error)?;
    let mut entries = Vec::new();
    let mut manual = Vec::new();
    for entry in index.entries {
        if cancel.is_cancelled() {
            return Err(PackSyncError::Cancelled);
        }
        // Index paths are relative to the index's folder, which is also
        // where they go in the game folder (at the alias, when there is
        // one); reads are relative to `pack.toml`.
        let in_pack = beside(&pack.index, &entry.path)?;
        if !entry.metafile {
            let source = match source.locate(location, &in_pack)? {
                PackFileLocation::Url(url) => EntrySource::Download {
                    urls: vec![url],
                    checksum: entry.checksum,
                    size: None,
                },
                PackFileLocation::Path(path) => EntrySource::Local {
                    path,
                    checksum: entry.checksum,
                },
            };
            entries.push(PackEntry {
                path: entry.target,
                source,
                preserve: entry.preserve,
            });
            continue;
        }
        let raw = source
            .read_metafile(location, &in_pack, &entry.checksum)
            .await?;
        let Some(file) = Metafile::validate(raw, &entry.target).map_err(files_error)? else {
            continue;
        };
        match file.source {
            MetafileSource::Download { url } => entries.push(PackEntry {
                path: file.path,
                source: EntrySource::Download {
                    urls: vec![url],
                    checksum: file.checksum,
                    size: None,
                },
                preserve: entry.preserve,
            }),
            MetafileSource::CurseForge(file) => manual.push(file),
        }
    }
    // Two entries for one file (folders on macOS and Windows ignore case)
    // would silently overwrite each other.
    let mut targets = std::collections::BTreeSet::new();
    let paths = entries
        .iter()
        .map(|entry| &entry.path)
        .chain(manual.iter().map(|file| &file.path));
    for path in paths {
        if !targets.insert(path.as_str().to_lowercase()) {
            return Err(files_error(PackwizError::DuplicatePath(path.to_string())));
        }
    }
    Ok(PackwizContents {
        pack,
        entries,
        manual,
    })
}

fn files_error(error: PackwizError) -> PackSyncError {
    PackSyncError::Files(PackFileError::Packwiz(error))
}

/// `path` (relative to the index's folder) relative to the pack's folder.
fn beside(index: &RelativePath, path: &RelativePath) -> Result<RelativePath, PackSyncError> {
    match index.as_str().rsplit_once('/') {
        None => Ok(path.clone()),
        Some((folder, _)) => RelativePath::parse(&format!("{folder}/{path}"))
            .map_err(|_| files_error(PackwizError::UnsafePath(path.to_string()))),
    }
}

/// Shared test setup for the packwiz use cases.
#[cfg(test)]
pub(crate) mod fixtures {
    use std::collections::{BTreeMap, HashMap};
    use std::path::{Path, PathBuf};

    use sha2::{Digest, Sha256};

    use crate::application::modpack_files::FileSyncDeps;
    use crate::application::modpack_files::fixtures::World;
    use crate::domain::integrity::{Checksum, curseforge_fingerprint};
    use crate::domain::launch::RelativePath;
    use crate::domain::modpack::packwiz::{RawIndexEntry, RawMetafile, RawPackIndex, RawPackToml};
    use crate::domain::modpack::{PackFileError, PackFileLocation, PackLocation, PackwizSource};
    use crate::testing::{
        FakeFileSystem, FakePackFiles, FakePackwizSource, InMemoryPackManifestStore, sha1_hex,
        sha512_hex,
    };

    pub const BASE: &str = "https://example.com/pack/";

    pub fn sha256_hex(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    pub fn location() -> PackLocation {
        PackLocation::parse(&format!("{BASE}pack.toml")).unwrap()
    }

    pub fn pack_toml(version: Option<&str>, minecraft: &str) -> RawPackToml {
        RawPackToml {
            name: "Cinepol Pack".into(),
            version: version.map(str::to_owned),
            pack_format: Some("packwiz:1.1.0".into()),
            index_file: "index.toml".into(),
            index_hash_format: "sha256".into(),
            index_hash: sha256_hex(b"index"),
            versions: BTreeMap::from([
                ("minecraft".into(), minecraft.into()),
                ("fabric".into(), "0.16.5".into()),
            ]),
        }
    }

    /// A plain file of the pack, hashed with sha256 (the index's format).
    pub fn plain(path: &str, bytes: &[u8]) -> RawIndexEntry {
        RawIndexEntry {
            file: path.into(),
            hash: sha256_hex(bytes),
            hash_format: None,
            metafile: false,
            preserve: false,
            alias: None,
        }
    }

    /// A metafile index entry; its hash covers the metafile's name.
    pub fn meta(path: &str) -> RawIndexEntry {
        RawIndexEntry {
            file: path.into(),
            hash: sha1_hex(path.as_bytes()),
            hash_format: Some("sha1".into()),
            metafile: true,
            preserve: false,
            alias: None,
        }
    }

    /// A metafile downloading `filename` (whose content is `bytes`) from Modrinth.
    pub fn modrinth(filename: &str, bytes: &[u8], side: &str) -> RawMetafile {
        RawMetafile {
            name: filename.into(),
            filename: filename.into(),
            side: Some(side.into()),
            url: Some(cdn(filename)),
            hash_format: "sha512".into(),
            hash: sha512_hex(bytes),
            mode: None,
            curseforge: None,
        }
    }

    /// A CurseForge metafile for `filename` (whose content is `bytes`).
    pub fn curseforge(filename: &str, bytes: &[u8]) -> RawMetafile {
        RawMetafile {
            name: "Just Enough Items".into(),
            filename: filename.into(),
            side: Some("client".into()),
            url: None,
            hash_format: "murmur2".into(),
            hash: curseforge_fingerprint(bytes).to_string(),
            mode: Some("metadata:curseforge".into()),
            curseforge: Some((238_222, 5_101_366)),
        }
    }

    pub fn cdn(filename: &str) -> String {
        format!("https://cdn.modrinth.com/data/x/{filename}")
    }

    pub fn source(
        pack: RawPackToml,
        files: Vec<RawIndexEntry>,
        metafiles: Vec<(&str, RawMetafile)>,
    ) -> FakePackwizSource {
        let mut source = FakePackwizSource::new(
            pack,
            RawPackIndex {
                hash_format: "sha256".into(),
                files,
            },
        );
        source.metafiles = metafiles
            .into_iter()
            .map(|(path, file)| (path.to_owned(), file))
            .collect::<HashMap<_, _>>();
        source
    }

    pub fn file_deps<'a, D>(
        world: &'a World,
        downloader: &'a D,
    ) -> FileSyncDeps<'a, D, FakeFileSystem, FakePackFiles, InMemoryPackManifestStore> {
        FileSyncDeps {
            downloader,
            fs: &world.fs,
            files: &world.files,
            manifests: &world.manifests,
            paths: &world.paths,
        }
    }

    pub fn checksum(entry: &RawIndexEntry) -> Checksum {
        let format = entry.hash_format.as_deref().unwrap_or("sha256");
        let algorithm = crate::domain::integrity::HashAlgorithm::parse(format).unwrap();
        Checksum::parse(algorithm, &entry.hash).unwrap()
    }

    /// Serves a pack from a real local folder: plain files are located under `root`.
    pub struct LocalPack {
        pub inner: FakePackwizSource,
        pub root: PathBuf,
    }

    impl PackwizSource for LocalPack {
        async fn read_pack(&self, pack: &PackLocation) -> Result<RawPackToml, PackFileError> {
            self.inner.read_pack(pack).await
        }

        async fn read_index(
            &self,
            pack: &PackLocation,
            relative: &RelativePath,
            expected: &Checksum,
        ) -> Result<RawPackIndex, PackFileError> {
            self.inner.read_index(pack, relative, expected).await
        }

        async fn read_metafile(
            &self,
            pack: &PackLocation,
            relative: &RelativePath,
            expected: &Checksum,
        ) -> Result<RawMetafile, PackFileError> {
            self.inner.read_metafile(pack, relative, expected).await
        }

        fn locate(
            &self,
            _pack: &PackLocation,
            relative: &RelativePath,
        ) -> Result<PackFileLocation, PackFileError> {
            Ok(PackFileLocation::Path(
                relative.under(Path::new(&self.root)),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::application::modpack_files::fixtures::{FailingDownloader, World, rel};
    use crate::domain::integrity::{Checksum, HashAlgorithm};
    use crate::domain::loader::LoaderKind;
    use crate::domain::modpack::packwiz::RawIndexEntry;
    use crate::testing::{
        FakeDownloader, FakeFileSystem, FakePackFiles, FakePackwizSource,
        InMemoryInstanceRepository, InMemoryPackManifestStore, InMemoryServerListStore,
        RecordingReporter, sha1_hex,
    };

    const ID: &str = "cinepol-pack";

    type Deps<'a, W, D> = InstallPackwizDeps<
        'a,
        InMemoryInstanceRepository,
        InMemoryServerListStore,
        W,
        D,
        FakeFileSystem,
        FakePackFiles,
        InMemoryPackManifestStore,
    >;

    fn deps<'a, W, D>(world: &'a World, source: &'a W, downloader: &'a D) -> Deps<'a, W, D> {
        InstallPackwizDeps {
            instances: &world.instances,
            servers: &world.servers,
            source,
            files: file_deps(world, downloader),
        }
    }

    fn request(pack: &PackLocation) -> InstallPackwizRequest<'_> {
        InstallPackwizRequest {
            pack,
            name: None,
            seed_server: None,
            claim: None,
        }
    }

    async fn install<W: PackwizSource, D: Downloader>(
        world: &World,
        source: &W,
        downloader: &D,
        pack: &PackLocation,
        cancel: &CancellationToken,
    ) -> Result<InstalledPackwiz, InstallPackwizError> {
        install_packwiz(
            request(pack),
            &deps(world, source, downloader),
            &RecordingReporter::default(),
            cancel,
        )
        .await
    }

    fn url_pack() -> (FakePackwizSource, FakeDownloader) {
        let source = source(
            pack_toml(Some("1.2.0"), "1.21.1"),
            vec![
                plain("config/sodium.toml", b"quality = 'high'"),
                meta("mods/sodium.pw.toml"),
                meta("mods/server-only.pw.toml"),
                meta("mods/jei.pw.toml"),
            ],
            vec![
                (
                    "mods/sodium.pw.toml",
                    modrinth("sodium.jar", b"sodium", "both"),
                ),
                (
                    "mods/server-only.pw.toml",
                    modrinth("server-only.jar", b"server", "server"),
                ),
                ("mods/jei.pw.toml", curseforge("jei.jar", b"jei")),
            ],
        );
        let downloader = FakeDownloader::with([
            (
                format!("{BASE}config/sodium.toml"),
                b"quality = 'high'".to_vec(),
            ),
            (cdn("sodium.jar"), b"sodium".to_vec()),
            (cdn("server-only.jar"), b"server".to_vec()),
        ]);
        (source, downloader)
    }

    #[tokio::test]
    async fn installs_a_url_pack_into_a_new_instance() {
        let world = World::new(vec![]);
        let (source, downloader) = url_pack();
        let installed = install(
            &world,
            &source,
            &downloader,
            &location(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        let instance = &installed.instance;
        assert_eq!(instance.id.as_str(), ID);
        assert_eq!(instance.name.as_str(), "Cinepol Pack");
        assert_eq!(instance.game_version.as_str(), "1.21.1");
        assert_eq!(instance.loader.kind(), LoaderKind::Fabric);
        assert_eq!(instance.loader.version().unwrap().as_str(), "0.16.5");
        assert_eq!(
            instance.origin,
            InstanceOrigin::Modpack(ModpackRef {
                source: ModpackSourceKind::Packwiz,
                project: format!("{BASE}pack.toml"),
                version: "1.2.0".into(),
            })
        );

        // The index and every metafile were read verified with their hashes.
        let verified = source.verified();
        let index = &source.index.files;
        assert_eq!(
            verified,
            [
                (
                    "index.toml".to_owned(),
                    Checksum::parse(HashAlgorithm::Sha256, &sha256_hex(b"index")).unwrap()
                ),
                ("mods/sodium.pw.toml".into(), checksum(&index[1])),
                ("mods/server-only.pw.toml".into(), checksum(&index[2])),
                ("mods/jei.pw.toml".into(), checksum(&index[3])),
            ]
        );

        // Client files are downloaded into staging with their declared hash;
        // the server-only file and the CurseForge one are not downloaded.
        let requests = downloader.requests();
        assert_eq!(requests.len(), 2);
        let config = &requests[0];
        assert_eq!(config.url, format!("{BASE}config/sodium.toml"));
        assert_eq!(
            config.checksum.as_ref().unwrap().algorithm(),
            HashAlgorithm::Sha256
        );
        assert_eq!(
            config.checksum.as_ref().unwrap().value(),
            sha256_hex(b"quality = 'high'")
        );
        let sodium = &requests[1];
        assert_eq!(sodium.url, cdn("sodium.jar"));
        assert_eq!(
            sodium.checksum.as_ref().unwrap().value(),
            crate::testing::sha512_hex(b"sodium")
        );
        for (request, path) in [(config, "config/sodium.toml"), (sodium, "mods/sodium.jar")] {
            assert!(request.destination.starts_with(world.paths.temp_dir()));
            assert!(request.destination.ends_with(path), "{request:?}");
        }
        assert_eq!(
            world.read_game_file(ID, "config/sodium.toml").unwrap(),
            b"quality = 'high'"
        );
        assert_eq!(
            world.read_game_file(ID, "mods/sodium.jar").unwrap(),
            b"sodium"
        );
        assert!(world.read_game_file(ID, "mods/server-only.jar").is_none());
        assert!(world.read_game_file(ID, "mods/jei.jar").is_none());

        // The CurseForge file is a manual download.
        assert_eq!(installed.pending.len(), 1);
        let jei = &installed.pending[0];
        assert_eq!(jei.path, rel("mods/jei.jar"));
        assert_eq!(jei.filename, "jei.jar");
        assert_eq!(jei.project_id, 238_222);
        assert_eq!(jei.checksum.algorithm(), HashAlgorithm::Murmur2);
        assert_eq!(
            installed.files,
            SyncSummary {
                written: 2,
                ..Default::default()
            }
        );
        let manifest = world.manifests.load(&instance.id).await.unwrap();
        assert_eq!(manifest.pending, installed.pending);
        assert_eq!(
            manifest.files.get(&rel("mods/sodium.jar")),
            Some(&sha1_hex(b"sodium"))
        );
    }

    #[tokio::test]
    async fn aliased_entries_install_at_their_alias() {
        let world = World::new(vec![]);
        let source = source(
            pack_toml(Some("1.0"), "1.21.1"),
            vec![
                RawIndexEntry {
                    alias: Some("options.txt".into()),
                    ..plain("defaults/options.txt", b"fov:90")
                },
                RawIndexEntry {
                    alias: Some("shaderpacks/sodium.pw.toml".into()),
                    ..meta("mods/sodium.pw.toml")
                },
            ],
            vec![(
                "mods/sodium.pw.toml",
                modrinth("sodium.jar", b"sodium", "both"),
            )],
        );
        let downloader = FakeDownloader::with([
            (format!("{BASE}defaults/options.txt"), b"fov:90".to_vec()),
            (cdn("sodium.jar"), b"sodium".to_vec()),
        ]);
        install(
            &world,
            &source,
            &downloader,
            &location(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        // Read from the listed path, installed at the alias (a metafile's
        // file lands in the alias's folder).
        assert_eq!(world.read_game_file(ID, "options.txt").unwrap(), b"fov:90");
        assert!(world.read_game_file(ID, "defaults/options.txt").is_none());
        assert_eq!(
            world.read_game_file(ID, "shaderpacks/sodium.jar").unwrap(),
            b"sodium"
        );
        assert!(world.read_game_file(ID, "mods/sodium.jar").is_none());
        let manifest = world
            .manifests
            .load(&InstanceId::parse(ID).unwrap())
            .await
            .unwrap();
        assert!(manifest.files.contains_key(&rel("options.txt")));
        assert!(manifest.files.contains_key(&rel("shaderpacks/sodium.jar")));
    }

    #[tokio::test]
    async fn a_pack_without_a_version_is_recorded_as_unversioned() {
        let world = World::new(vec![]);
        let source = source(pack_toml(None, "1.21.1"), vec![], vec![]);
        let installed = install(
            &world,
            &source,
            &FakeDownloader::default(),
            &location(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let InstanceOrigin::Modpack(origin) = installed.instance.origin else {
            panic!("expected a modpack origin");
        };
        assert_eq!(origin.version, UNVERSIONED);
    }

    #[tokio::test]
    async fn refuses_two_files_that_land_on_the_same_path() {
        let world = World::new(vec![]);
        let source = source(
            pack_toml(Some("1.0"), "1.21.1"),
            vec![meta("mods/a.pw.toml"), meta("mods/b.pw.toml")],
            vec![
                ("mods/a.pw.toml", modrinth("sodium.jar", b"one", "both")),
                ("mods/b.pw.toml", modrinth("Sodium.jar", b"two", "both")),
            ],
        );
        let result = install(
            &world,
            &source,
            &FakeDownloader::default(),
            &location(),
            &CancellationToken::new(),
        )
        .await;
        assert!(
            matches!(
                result,
                Err(InstallPackwizError::Pack(PackSyncError::Files(
                    PackFileError::Packwiz(PackwizError::DuplicatePath(_))
                )))
            ),
            "{result:?}"
        );
        assert!(world.instances.list().await.unwrap().is_empty());
    }

    fn local_pack(world: &World, content: &[u8]) -> LocalPack {
        let root = world.paths.temp_dir().join("my-pack");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("config/options.toml"), content).unwrap();
        LocalPack {
            inner: source(
                pack_toml(Some("2.0"), "1.21.1"),
                vec![plain("config/options.toml", b"declared")],
                vec![],
            ),
            root,
        }
    }

    #[tokio::test]
    async fn a_local_pack_copies_its_files_after_verifying_them() {
        let world = World::new(vec![]);
        let source = local_pack(&world, b"declared");
        let location = PackLocation::parse("/home/player/my-pack/pack.toml").unwrap();
        let installed = install(
            &world,
            &source,
            &FakeDownloader::default(),
            &location,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            world.read_game_file(ID, "config/options.toml").unwrap(),
            b"declared"
        );
        assert!(source.root.join("config/options.toml").exists());
        let InstanceOrigin::Modpack(origin) = installed.instance.origin else {
            panic!("expected a modpack origin");
        };
        assert_eq!(origin.project, "/home/player/my-pack/pack.toml");
    }

    #[tokio::test]
    async fn a_local_file_with_the_wrong_hash_fails_and_removes_the_instance() {
        let world = World::new(vec![]);
        let source = local_pack(&world, b"tampered");
        let location = PackLocation::parse("/home/player/my-pack/pack.toml").unwrap();
        let result = install(
            &world,
            &source,
            &FakeDownloader::default(),
            &location,
            &CancellationToken::new(),
        )
        .await;
        assert!(
            matches!(
                result,
                Err(InstallPackwizError::Pack(PackSyncError::Files(
                    PackFileError::HashMismatch(_)
                )))
            ),
            "{result:?}"
        );
        assert!(world.instances.list().await.unwrap().is_empty());
        assert!(world.manifests.manifests.lock().unwrap().is_empty());
        assert!(world.read_game_file(ID, "config/options.toml").is_none());
    }

    #[tokio::test]
    async fn a_failed_sync_saves_no_manifest_and_removes_the_new_instance() {
        let world = World::new(vec![]);
        let (source, _) = url_pack();
        let result = install(
            &world,
            &source,
            &FailingDownloader,
            &location(),
            &CancellationToken::new(),
        )
        .await;
        assert!(matches!(
            result,
            Err(InstallPackwizError::Pack(PackSyncError::Download(_)))
        ));
        let id = InstanceId::parse(ID).unwrap();
        assert!(world.manifests.manifests.lock().unwrap().is_empty());
        assert!(world.instances.list().await.unwrap().is_empty());
        assert!(world.fs.removed().contains(&world.paths.instance_dir(&id)));
    }

    #[tokio::test]
    async fn cancellation_stops_before_downloading_or_creating_anything() {
        let world = World::new(vec![]);
        let (source, downloader) = url_pack();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = install(&world, &source, &downloader, &location(), &cancel).await;
        assert_eq!(result, Err(InstallPackwizError::Cancelled));
        assert!(downloader.requests().is_empty());
        assert!(world.instances.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_busy_instance_aborts_the_install_and_removes_it() {
        let world = World::new(vec![]);
        let (source, downloader) = url_pack();
        let claim = |_: &InstanceId| false;
        let pack = location();
        let request = InstallPackwizRequest {
            claim: Some(&claim),
            ..request(&pack)
        };
        let result = install_packwiz(
            request,
            &deps(&world, &source, &downloader),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(
            result,
            Err(InstallPackwizError::Busy(InstanceId::parse(ID).unwrap()))
        );
        assert!(world.instances.list().await.unwrap().is_empty());
        assert!(downloader.requests().is_empty());
    }

    #[test]
    fn index_paths_are_read_beside_a_nested_index() {
        assert_eq!(
            beside(&rel("pack/index.toml"), &rel("mods/a.pw.toml")).unwrap(),
            rel("pack/mods/a.pw.toml")
        );
        assert_eq!(
            beside(&rel("index.toml"), &rel("mods/a.pw.toml")).unwrap(),
            rel("mods/a.pw.toml")
        );
    }
}
