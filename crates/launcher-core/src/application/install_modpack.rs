//! Installs a modpack into a new instance: from a Modrinth catalog version or
//! from a local `.mrpack` file.
//!
//! This creates the instance and writes the pack's files into its game
//! folder. It does not install Minecraft or the loader; `install_instance`
//! does that afterwards.

use std::path::Path;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::create_instance::{CreateInstanceError, NewInstance, create_instance};
use crate::application::modpack_files::{
    PackSyncDeps, PackSyncError, SyncSummary, download_pack, release_dir, scratch_dir, sync_pack,
};
use crate::domain::instance::{Instance, InstanceId, InstanceOrigin, InstanceRepository};
use crate::domain::modpack::sync::PackManifest;
use crate::domain::modpack::{
    ModpackRef, ModpackSource, ModpackSourceError, ModpackSourceKind, MrpackArchive, PackFileError,
    PackFiles, PackManifestStore,
};
use crate::domain::ports::{Downloader, FileSystem, ProgressReporter};
use crate::domain::server_list::{ServerEntry, ServerListStore};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstallModpackError {
    #[error(transparent)]
    Source(#[from] ModpackSourceError),
    #[error("{0:?} modpacks cannot be installed yet")]
    UnsupportedSource(ModpackSourceKind),
    #[error("modpack version {0} was not found")]
    VersionNotFound(String),
    #[error("modpack version {0} has no .mrpack file")]
    NoPackFile(String),
    #[error(transparent)]
    Create(#[from] CreateInstanceError),
    #[error(transparent)]
    Pack(PackSyncError),
    #[error("the modpack installation was cancelled")]
    Cancelled,
}

impl From<PackSyncError> for InstallModpackError {
    fn from(error: PackSyncError) -> Self {
        match error {
            PackSyncError::Cancelled => Self::Cancelled,
            other => Self::Pack(other),
        }
    }
}

impl From<PackFileError> for InstallModpackError {
    fn from(error: PackFileError) -> Self {
        Self::Pack(PackSyncError::Files(error))
    }
}

/// Which pack to install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackInput<'a> {
    /// A version of a Modrinth project, by ids.
    Catalog { project: &'a str, version: &'a str },
    /// A `.mrpack` file on disk.
    File(&'a Path),
}

pub struct InstallModpackRequest<'a> {
    pub pack: PackInput<'a>,
    /// The instance name; the pack's name when `None` or blank.
    pub name: Option<&'a str>,
    /// Put first in the new instance's `servers.dat` (the CINEPOL server).
    pub seed_server: Option<ServerEntry>,
}

/// Everything the modpack installer talks to.
pub struct InstallModpackDeps<'a, I, S, C, A, D, F, P, M> {
    pub instances: &'a I,
    pub servers: &'a S,
    pub source: &'a C,
    pub pack: PackSyncDeps<'a, A, D, F, P, M>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledModpack {
    pub instance: Instance,
    pub files: SyncSummary,
}

/// A pack from the catalog gets a [`InstanceOrigin::Modpack`] origin so it can
/// be updated later; a local file gets [`InstanceOrigin::Custom`] (it can
/// still be updated from another file, because its manifest is stored).
///
/// When writing the pack's files fails, the new instance is deleted again
/// (metadata and folder) so a failed import leaves nothing half-made.
#[allow(clippy::too_many_arguments)]
pub async fn install_modpack<I, S, C, A, D, F, P, M, R>(
    request: InstallModpackRequest<'_>,
    deps: &InstallModpackDeps<'_, I, S, C, A, D, F, P, M>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<InstalledModpack, InstallModpackError>
where
    I: InstanceRepository,
    S: ServerListStore,
    C: ModpackSource,
    A: MrpackArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let (project, version) = match request.pack {
        PackInput::Catalog { project, version } => (project, version),
        PackInput::File(archive) => {
            let origin = InstanceOrigin::Custom;
            return install_from(archive, origin, request, deps, reporter, cancel).await;
        }
    };
    if deps.source.kind() != ModpackSourceKind::Modrinth {
        return Err(InstallModpackError::UnsupportedSource(deps.source.kind()));
    }
    let versions = deps.source.versions(project).await?;
    let found = versions
        .iter()
        .find(|candidate| candidate.id == version)
        .ok_or_else(|| InstallModpackError::VersionNotFound(version.to_owned()))?;
    let file = found
        .pack_file
        .as_ref()
        .ok_or_else(|| InstallModpackError::NoPackFile(version.to_owned()))?;
    let origin = InstanceOrigin::Modpack(ModpackRef {
        source: ModpackSourceKind::Modrinth,
        project: project.to_owned(),
        version: found.id.clone(),
    });

    let dir = scratch_dir(deps.pack.paths, "download");
    let result = async {
        let archive = download_pack(deps.pack.downloader, file, &dir, reporter, cancel).await?;
        install_from(&archive, origin, request, deps, reporter, cancel).await
    }
    .await;
    release_dir(deps.pack.fs, &dir).await;
    result
}

#[allow(clippy::too_many_arguments)]
async fn install_from<I, S, C, A, D, F, P, M, R>(
    archive: &Path,
    origin: InstanceOrigin,
    request: InstallModpackRequest<'_>,
    deps: &InstallModpackDeps<'_, I, S, C, A, D, F, P, M>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<InstalledModpack, InstallModpackError>
where
    I: InstanceRepository,
    S: ServerListStore,
    A: MrpackArchive,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let index = deps.pack.archive.read_index(archive).await?;
    if cancel.is_cancelled() {
        return Err(InstallModpackError::Cancelled);
    }
    let name = request
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&index.name);
    let instance = create_instance(
        NewInstance {
            name,
            game_version: index.game_version.as_str(),
            loader: index.loader.clone(),
            origin,
            seed_server: request.seed_server,
        },
        deps.instances,
        deps.servers,
    )
    .await?;
    let synced = sync_pack(
        &deps.pack,
        &instance.id,
        archive,
        &index,
        &PackManifest::default(),
        reporter,
        cancel,
    )
    .await;
    match synced {
        Ok(files) => Ok(InstalledModpack { instance, files }),
        Err(error) => {
            discard(deps, &instance.id).await;
            Err(error.into())
        }
    }
}

/// Removes a half-installed instance: its metadata and its folder.
async fn discard<I, S, C, A, D, F, P, M>(
    deps: &InstallModpackDeps<'_, I, S, C, A, D, F, P, M>,
    id: &InstanceId,
) where
    I: InstanceRepository,
    F: FileSystem,
{
    if let Err(error) = deps.instances.delete(id).await {
        tracing::warn!(instance = %id, %error, "could not delete a failed modpack instance");
    }
    release_dir(deps.pack.fs, &deps.pack.paths.instance_dir(id)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::modpack_files::fixtures::*;
    use crate::domain::loader::LoaderKind;
    use crate::domain::server_list::ServerAddress;
    use crate::testing::{
        FakeFileSystem, FakeModpackSource, FakePackFiles, InMemoryInstanceRepository,
        InMemoryPackManifestStore, InMemoryServerListStore, RecordingReporter, sha1_hex,
        sha512_hex,
    };

    const LOCAL: &str = "/packs/fo.mrpack";

    fn v1_pack(key: &str) -> Pack {
        (
            key.into(),
            index(
                "v1",
                "26.2",
                "0.19.5",
                vec![
                    entry("v1", "mods/sodium.jar", "required"),
                    entry("v1", "mods/zoomify.jar", "optional"),
                    entry("v1", "mods/server-only.jar", "unsupported"),
                    entry("v1", "config/sodium.json", "required"),
                ],
            ),
            vec![
                ("config/sodium.json", b"override config".as_slice()),
                ("options.txt", b"lang:es_es".as_slice()),
            ],
        )
    }

    type Deps<'a, D> = InstallModpackDeps<
        'a,
        InMemoryInstanceRepository,
        InMemoryServerListStore,
        FakeModpackSource,
        TestArchive,
        D,
        FakeFileSystem,
        FakePackFiles,
        InMemoryPackManifestStore,
    >;

    fn deps<'a, D>(world: &'a World, downloader: &'a D) -> Deps<'a, D> {
        InstallModpackDeps {
            instances: &world.instances,
            servers: &world.servers,
            source: &world.source,
            pack: world.pack_deps(downloader),
        }
    }

    fn local(name: Option<&str>) -> InstallModpackRequest<'_> {
        InstallModpackRequest {
            pack: PackInput::File(Path::new(LOCAL)),
            name,
            seed_server: None,
        }
    }

    async fn install(
        world: &World,
        request: InstallModpackRequest<'_>,
    ) -> Result<InstalledModpack, InstallModpackError> {
        install_modpack(
            request,
            &deps(world, &world.downloader),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
    }

    const ID: &str = "fabulously-optimized";

    #[tokio::test]
    async fn installs_a_local_pack_into_a_new_instance() {
        let world = World::new(vec![v1_pack(LOCAL)]);
        let installed = install(&world, local(None)).await.unwrap();

        let instance = &installed.instance;
        assert_eq!(instance.id.as_str(), ID);
        assert_eq!(instance.name.as_str(), "Fabulously Optimized");
        assert_eq!(instance.game_version.as_str(), "26.2");
        assert_eq!(instance.loader.kind(), LoaderKind::Fabric);
        assert_eq!(instance.loader.version().unwrap().as_str(), "0.19.5");
        assert_eq!(instance.origin, InstanceOrigin::Custom);
        assert_eq!(
            world.instances.list().await.unwrap(),
            std::slice::from_ref(instance)
        );

        // Index files are downloaded and verified against their sha512; the
        // server-only file and the one an override replaces are not.
        let requests = world.downloader.requests();
        let downloaded: Vec<_> = requests
            .iter()
            .map(|request| request.destination.clone())
            .collect();
        // They land in the launcher's temp folder first, then move into the game folder.
        assert_eq!(downloaded.len(), 2);
        for (destination, path) in downloaded
            .iter()
            .zip(["mods/sodium.jar", "mods/zoomify.jar"])
        {
            assert!(
                destination.starts_with(world.paths.temp_dir()),
                "{destination:?}"
            );
            assert!(destination.ends_with(path), "{destination:?}");
            assert_eq!(
                world.read_game_file(ID, path).unwrap(),
                url("v1", path).into_bytes()
            );
        }
        let sodium = url("v1", "mods/sodium.jar");
        assert_eq!(requests[0].url, sodium);
        assert_eq!(
            requests[0].checksum.as_ref().unwrap().value(),
            sha512_hex(sodium.as_bytes())
        );
        assert_eq!(requests[0].size, Some(sodium.len() as u64));

        // Overrides are placed, replacing the index file at the same path.
        assert_eq!(
            world.read_game_file(ID, "config/sodium.json").unwrap(),
            b"override config"
        );
        assert_eq!(
            world.read_game_file(ID, "options.txt").unwrap(),
            b"lang:es_es"
        );
        assert!(world.read_game_file(ID, "mods/server-only.jar").is_none());

        let manifest = world.manifests.load(&instance.id).await.unwrap();
        let recorded: Vec<(&str, &str)> = manifest
            .files
            .iter()
            .map(|(path, sha1)| (path.as_str(), sha1.as_str()))
            .collect();
        let sha1 = |bytes: &[u8]| sha1_hex(bytes);
        assert_eq!(
            recorded,
            [
                ("config/sodium.json", sha1(b"override config").as_str()),
                ("mods/sodium.jar", sha1(sodium.as_bytes()).as_str()),
                (
                    "mods/zoomify.jar",
                    sha1(url("v1", "mods/zoomify.jar").as_bytes()).as_str()
                ),
                ("options.txt", sha1(b"lang:es_es").as_str()),
            ]
        );
        assert_eq!(
            installed.files,
            SyncSummary {
                written: 4,
                ..Default::default()
            }
        );
        // The per-run staging folder is cleaned up.
        let removed = world.fs.removed();
        assert_eq!(removed.len(), 1);
        assert!(removed[0].starts_with(world.paths.temp_dir()));
    }

    #[tokio::test]
    async fn uses_the_given_name_and_seeds_the_server_list() {
        let world = World::new(vec![v1_pack(LOCAL)]);
        let entry = ServerEntry::new(
            "CINEPOL",
            ServerAddress::parse("mc.policarpiers.com").unwrap(),
        );
        let request = InstallModpackRequest {
            seed_server: Some(entry.clone()),
            ..local(Some("My Pack"))
        };
        let installed = install(&world, request).await.unwrap();
        assert_eq!(installed.instance.name.as_str(), "My Pack");
        assert_eq!(installed.instance.id.as_str(), "my-pack");
        let servers = world.servers.load(&installed.instance.id).await.unwrap();
        assert_eq!(servers.entries(), [entry]);
    }

    #[tokio::test]
    async fn installs_a_catalog_version_after_downloading_its_verified_pack() {
        let mut world = World::new(vec![v1_pack(&pack_url("v1"))]);
        world.source.versions.insert(
            "fo".into(),
            vec![catalog_version("v2", false), catalog_version("v1", true)],
        );
        let request = InstallModpackRequest {
            pack: PackInput::Catalog {
                project: "fo",
                version: "v1",
            },
            name: None,
            seed_server: None,
        };
        let installed = install(&world, request).await.unwrap();

        assert_eq!(
            installed.instance.origin,
            InstanceOrigin::Modpack(ModpackRef {
                source: ModpackSourceKind::Modrinth,
                project: "fo".into(),
                version: "v1".into(),
            })
        );
        let requests = world.downloader.requests();
        let pack = &requests[0];
        assert_eq!(pack.url, pack_url("v1"));
        assert_eq!(pack.checksum, Some(pack_file("v1").checksum));
        assert_eq!(pack.size, Some(pack_file("v1").size));
        assert!(pack.destination.starts_with(world.paths.temp_dir()));
        assert_eq!(requests.len(), 3, "the pack, then its two client mods");
        // The downloaded pack's folder is removed with the staging folder.
        let removed = world.fs.removed();
        assert_eq!(removed.len(), 2);
        assert!(removed.iter().any(|dir| pack.destination.starts_with(dir)));
    }

    #[tokio::test]
    async fn refuses_a_catalog_version_without_a_pack_file() {
        let mut world = World::new(vec![]);
        world
            .source
            .versions
            .insert("fo".into(), vec![catalog_version("v2", false)]);
        let request = |version| InstallModpackRequest {
            pack: PackInput::Catalog {
                project: "fo",
                version,
            },
            name: None,
            seed_server: None,
        };
        assert_eq!(
            install(&world, request("v2")).await,
            Err(InstallModpackError::NoPackFile("v2".into()))
        );
        assert_eq!(
            install(&world, request("v9")).await,
            Err(InstallModpackError::VersionNotFound("v9".into()))
        );
        assert!(world.downloader.requests().is_empty());
        assert!(world.instances.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancellation_stops_before_downloading_or_creating_anything() {
        let world = World::new(vec![v1_pack(LOCAL)]);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = install_modpack(
            local(None),
            &deps(&world, &world.downloader),
            &RecordingReporter::default(),
            &cancel,
        )
        .await;
        assert_eq!(result, Err(InstallModpackError::Cancelled));
        assert!(world.downloader.requests().is_empty());
        assert!(world.instances.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failed_sync_saves_no_manifest_and_removes_the_new_instance() {
        let world = World::new(vec![v1_pack(LOCAL)]);
        let result = install_modpack(
            local(None),
            &deps(&world, &FailingDownloader),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;
        assert!(matches!(
            result,
            Err(InstallModpackError::Pack(PackSyncError::Download(_)))
        ));
        let id = InstanceId::parse(ID).unwrap();
        assert!(world.manifests.manifests.lock().unwrap().is_empty());
        assert!(world.instances.list().await.unwrap().is_empty());
        assert!(world.fs.removed().contains(&world.paths.instance_dir(&id)));
    }

    #[tokio::test]
    async fn never_installs_into_a_folder_that_already_exists() {
        // An instance folder whose instance.json cannot be read is not listed,
        // but its worlds still belong to the player.
        let world = World::new(vec![v1_pack(LOCAL)]);
        world.instances.occupy(InstanceId::parse(ID).unwrap());
        world.write_game_file(ID, "saves/My World/level.dat", b"years of building");

        let failed = install_modpack(
            local(None),
            &deps(&world, &FailingDownloader),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await;
        assert!(failed.is_err());
        let installed = install(&world, local(None)).await.unwrap();

        assert_ne!(installed.instance.id.as_str(), ID);
        assert_eq!(
            world
                .read_game_file(ID, "saves/My World/level.dat")
                .as_deref(),
            Some(b"years of building".as_slice())
        );
        assert!(
            !world
                .fs
                .removed()
                .contains(&world.paths.instance_dir(&InstanceId::parse(ID).unwrap()))
        );
    }

    #[tokio::test]
    async fn records_the_sha1_of_the_written_file_not_the_declared_one() {
        let mut wrong = entry("v1", "mods/sodium.jar", "required");
        wrong.hashes.insert("sha1".into(), "0".repeat(40));
        let pack = (
            LOCAL.into(),
            index("v1", "26.2", "0.19.5", vec![wrong]),
            vec![],
        );
        let world = World::new(vec![pack]);
        let installed = install(&world, local(None)).await.unwrap();

        let manifests = world.manifests.manifests.lock().unwrap();
        let manifest = &manifests[&installed.instance.id];
        let written = world
            .read_game_file(installed.instance.id.as_str(), "mods/sodium.jar")
            .unwrap();
        assert_eq!(manifest.files[&rel("mods/sodium.jar")], sha1_hex(&written));
    }
}
