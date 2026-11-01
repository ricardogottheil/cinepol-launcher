//! Updates an instance installed from a packwiz pack to the pack's current
//! state, keeping the player's changes. Meant to run before each launch
//! ("update on launch"): the sync is idempotent, so an unchanged pack
//! downloads nothing.

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::install_packwiz::{
    download_curseforge_files, packwiz_origin, read_packwiz,
};
use crate::application::modpack_files::{FileSyncDeps, PackSyncError, SyncSummary, sync_files};
use crate::domain::instance::{
    Instance, InstanceId, InstanceOrigin, InstanceRepository, InstanceRepositoryError,
};
use crate::domain::modpack::sync::ManualFile;
use crate::domain::modpack::{
    CurseForgeLookup, ModpackSourceKind, PackFileError, PackFiles, PackLocation, PackManifestStore,
    PackwizSource,
};
use crate::domain::ports::{Downloader, FileSystem, ProgressReporter};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdatePackwizError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error("instance {0} was not installed from a packwiz pack")]
    NotPackwiz(InstanceId),
    #[error(transparent)]
    Pack(PackSyncError),
    #[error("the modpack update was cancelled")]
    Cancelled,
}

impl From<PackSyncError> for UpdatePackwizError {
    fn from(error: PackSyncError) -> Self {
        match error {
            PackSyncError::Cancelled => Self::Cancelled,
            other => Self::Pack(other),
        }
    }
}

impl From<PackFileError> for UpdatePackwizError {
    fn from(error: PackFileError) -> Self {
        Self::Pack(PackSyncError::Files(error))
    }
}

/// Everything the packwiz updater talks to.
pub struct UpdatePackwizDeps<'a, I, W, L, D, F, P, M> {
    pub instances: &'a I,
    pub source: &'a W,
    /// Resolves CurseForge-mode files; without a key they stay manual.
    pub curseforge: &'a L,
    pub files: FileSyncDeps<'a, D, F, P, M>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatedPackwiz {
    pub instance: Instance,
    /// Nothing written or removed means the instance was already up to date.
    pub files: SyncSummary,
    /// Files the player must still download by hand.
    pub pending: Vec<ManualFile>,
}

/// Re-reads the pack from the location stored in the instance origin. Only
/// pack files that are still intact are replaced or removed, preserved files
/// are never replaced, and the instance's game version, loader and origin
/// version follow the pack.
pub async fn update_packwiz<I, W, L, D, F, P, M, R>(
    id: &InstanceId,
    deps: &UpdatePackwizDeps<'_, I, W, L, D, F, P, M>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<UpdatedPackwiz, UpdatePackwizError>
where
    I: InstanceRepository,
    W: PackwizSource,
    L: CurseForgeLookup,
    D: Downloader,
    F: FileSystem,
    P: PackFiles,
    M: PackManifestStore,
    R: ProgressReporter,
{
    let instance = deps.instances.get(id).await?;
    let location = match &instance.origin {
        InstanceOrigin::Modpack(pack) if pack.source == ModpackSourceKind::Packwiz => {
            PackLocation::parse(&pack.project)?
        }
        _ => return Err(UpdatePackwizError::NotPackwiz(instance.id)),
    };
    let mut contents = read_packwiz(deps.source, &location, cancel).await?;
    download_curseforge_files(deps.curseforge, &mut contents).await?;
    let previous = deps.files.manifests.load(&instance.id).await?;
    let synced = sync_files(
        &deps.files,
        &instance.id,
        &contents.entries,
        &contents.manual,
        &previous,
        reporter,
        cancel,
    )
    .await?;

    // Re-read: the player may have edited the settings during the sync.
    let mut instance = deps.instances.get(&instance.id).await?;
    let before = instance.clone();
    instance.game_version = contents.pack.game_version.clone();
    instance.loader = contents.pack.loader.clone();
    instance.origin = packwiz_origin(&location, &contents.pack);
    if instance != before {
        deps.instances.save(&instance).await?;
    }
    Ok(UpdatedPackwiz {
        instance,
        files: synced.summary,
        pending: synced.pending,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::create_instance::{NewInstance, create_instance};
    use crate::application::install_packwiz::fixtures::*;
    use crate::application::install_packwiz::{
        InstallPackwizDeps, InstallPackwizRequest, install_packwiz,
    };
    use crate::application::modpack_files::fixtures::{FailingDownloader, World, rel};
    use crate::domain::loader::LoaderSpec;
    use crate::domain::modpack::ModpackRef;
    use crate::domain::modpack::curseforge::CurseForgeFile;
    use crate::domain::modpack::packwiz::RawIndexEntry;
    use crate::domain::mods::DISABLED_SUFFIX;
    use crate::testing::{
        FakeCurseForge, FakeDownloader, FakePackwizSource, RecordingReporter, sha1_hex,
    };

    const ID: &str = "cinepol-pack";

    fn id() -> InstanceId {
        InstanceId::parse(ID).unwrap()
    }

    fn preserved(path: &str, bytes: &[u8]) -> RawIndexEntry {
        RawIndexEntry {
            preserve: true,
            ..plain(path, bytes)
        }
    }

    /// Version 1.0: three config files (one preserved) and Sodium.
    fn v1() -> (FakePackwizSource, FakeDownloader) {
        let source = source(
            pack_toml(Some("1.0"), "1.21.1"),
            vec![
                plain("config/a.toml", b"a1"),
                plain("config/b.toml", b"b1"),
                preserved("config/keep.toml", b"k1"),
                meta("mods/sodium.pw.toml"),
            ],
            vec![(
                "mods/sodium.pw.toml",
                modrinth("sodium.jar", b"sodium", "both"),
            )],
        );
        let downloader = FakeDownloader::with([
            (format!("{BASE}config/a.toml"), b"a1".to_vec()),
            (format!("{BASE}config/b.toml"), b"b1".to_vec()),
            (format!("{BASE}config/keep.toml"), b"k1".to_vec()),
            (cdn("sodium.jar"), b"sodium".to_vec()),
        ]);
        (source, downloader)
    }

    /// Version 1.1: changes `a` and the preserved file, drops `b`, adds `c`
    /// and a CurseForge mod, and moves to Minecraft 1.21.2.
    fn v2() -> (FakePackwizSource, FakeDownloader) {
        let source = source(
            pack_toml(Some("1.1"), "1.21.2"),
            vec![
                plain("config/a.toml", b"a2"),
                plain("config/c.toml", b"c2"),
                preserved("config/keep.toml", b"k2"),
                meta("mods/sodium.pw.toml"),
                meta("mods/jei.pw.toml"),
            ],
            vec![
                (
                    "mods/sodium.pw.toml",
                    modrinth("sodium.jar", b"sodium", "both"),
                ),
                ("mods/jei.pw.toml", curseforge("jei.jar", b"jei")),
            ],
        );
        let downloader = FakeDownloader::with([
            (format!("{BASE}config/a.toml"), b"a2".to_vec()),
            (format!("{BASE}config/c.toml"), b"c2".to_vec()),
            (format!("{BASE}config/keep.toml"), b"k2".to_vec()),
            (cdn("sodium.jar"), b"sodium".to_vec()),
        ]);
        (source, downloader)
    }

    async fn install_v1(world: &World) {
        let (source, downloader) = v1();
        let pack = location();
        install_packwiz(
            InstallPackwizRequest {
                pack: &pack,
                name: None,
                seed_server: None,
                claim: None,
            },
            &InstallPackwizDeps {
                instances: &world.instances,
                servers: &world.servers,
                source: &source,
                curseforge: crate::testing::no_curseforge(),
                files: file_deps(world, &downloader),
            },
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    }

    async fn update<W: PackwizSource, D: Downloader>(
        world: &World,
        source: &W,
        downloader: &D,
        cancel: &CancellationToken,
    ) -> Result<UpdatedPackwiz, UpdatePackwizError> {
        let deps = UpdatePackwizDeps {
            instances: &world.instances,
            source,
            curseforge: crate::testing::no_curseforge(),
            files: file_deps(world, downloader),
        };
        update_packwiz(&id(), &deps, &RecordingReporter::default(), cancel).await
    }

    #[tokio::test]
    async fn keeps_player_changes_and_removes_intact_dropped_files() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        world.write_game_file(ID, "config/a.toml", b"player settings");
        world.write_game_file(ID, "mods/mine.jar", b"player mod");

        let (source, downloader) = v2();
        let updated = update(&world, &source, &downloader, &CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(
            updated.files,
            SyncSummary {
                written: 1,
                unchanged: 2,
                kept: 1,
                removed: 1,
            }
        );
        let read = |path| world.read_game_file(ID, path);
        assert_eq!(read("config/a.toml").unwrap(), b"player settings");
        assert!(read("config/b.toml").is_none());
        assert_eq!(read("config/c.toml").unwrap(), b"c2");
        assert_eq!(read("mods/mine.jar").unwrap(), b"player mod");
        // Sodium did not change, so it is not downloaded again.
        let urls: Vec<_> = downloader.requests().into_iter().map(|r| r.url).collect();
        assert_eq!(urls, [format!("{BASE}config/c.toml")]);

        assert_eq!(updated.pending.len(), 1);
        assert_eq!(updated.pending[0].path, rel("mods/jei.jar"));
        let instance = &updated.instance;
        assert_eq!(instance.game_version.as_str(), "1.21.2");
        assert_eq!(
            instance.origin,
            InstanceOrigin::Modpack(ModpackRef {
                source: ModpackSourceKind::Packwiz,
                project: format!("{BASE}pack.toml"),
                version: "1.1".into(),
            })
        );
        assert_eq!(world.instances.get(&id()).await.unwrap(), *instance);
        let manifest = world.manifests.load(&id()).await.unwrap();
        assert_eq!(manifest.pending, updated.pending);
        assert!(!manifest.files.contains_key(&rel("config/a.toml")));
        assert!(!manifest.files.contains_key(&rel("config/b.toml")));
    }

    #[tokio::test]
    async fn preserved_files_are_not_replaced_on_update() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        let (source, downloader) = v2();
        update(&world, &source, &downloader, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(world.read_game_file(ID, "config/keep.toml").unwrap(), b"k1");
        assert!(
            !downloader
                .requests()
                .iter()
                .any(|request| request.url.ends_with("keep.toml"))
        );
    }

    /// What turning a mod off does: rename it to `<name>.disabled`.
    fn disable(world: &World, path: &str) {
        let bytes = world.read_game_file(ID, path).unwrap();
        world.write_game_file(ID, &format!("{path}{DISABLED_SUFFIX}"), &bytes);
        std::fs::remove_file(world.game_file(ID, path)).unwrap();
    }

    /// Version 1.0 with Sodium shipped as `filename` (content `bytes`), or
    /// without Sodium.
    fn v1_with_sodium(sodium: Option<(&str, &[u8])>) -> (FakePackwizSource, FakeDownloader) {
        let mut index = vec![
            plain("config/a.toml", b"a1"),
            plain("config/b.toml", b"b1"),
            preserved("config/keep.toml", b"k1"),
        ];
        let mut metafiles = Vec::new();
        let mut downloads = vec![
            (format!("{BASE}config/a.toml"), b"a1".to_vec()),
            (format!("{BASE}config/b.toml"), b"b1".to_vec()),
            (format!("{BASE}config/keep.toml"), b"k1".to_vec()),
        ];
        if let Some((filename, bytes)) = sodium {
            index.push(meta("mods/sodium.pw.toml"));
            metafiles.push((
                "mods/sodium.pw.toml",
                modrinth_in("sodium", filename, bytes, "both"),
            ));
            downloads.push((cdn_in("sodium", filename), bytes.to_vec()));
        }
        let source = source(pack_toml(Some("1.0.1"), "1.21.1"), index, metafiles);
        (source, FakeDownloader::with(downloads))
    }

    async fn update_to(world: &World, sodium: Option<(&str, &[u8])>) -> UpdatedPackwiz {
        let (source, downloader) = v1_with_sodium(sodium);
        update(world, &source, &downloader, &CancellationToken::new())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_pack_mod_the_player_disabled_stays_disabled() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        disable(&world, "mods/sodium.jar");

        let (source, downloader) = v1();
        let updated = update(&world, &source, &downloader, &CancellationToken::new())
            .await
            .unwrap();

        assert!(
            world.read_game_file(ID, "mods/sodium.jar").is_none(),
            "not enabled again"
        );
        assert_eq!(
            world
                .read_game_file(ID, "mods/sodium.jar.disabled")
                .unwrap(),
            b"sodium"
        );
        assert!(downloader.requests().is_empty(), "nothing downloaded");
        assert_eq!(updated.files.unchanged, 4);
        let manifest = world.manifests.load(&id()).await.unwrap();
        assert!(
            manifest.files.contains_key(&rel("mods/sodium.jar")),
            "still the pack's file"
        );
    }

    #[tokio::test]
    async fn a_disabled_pack_mod_gets_the_new_version_and_stays_off() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        disable(&world, "mods/sodium.jar");

        let updated = update_to(&world, Some(("sodium.jar", b"sodium 2"))).await;

        assert!(world.read_game_file(ID, "mods/sodium.jar").is_none());
        assert_eq!(
            world
                .read_game_file(ID, "mods/sodium.jar.disabled")
                .unwrap(),
            b"sodium 2"
        );
        assert_eq!(updated.files.written, 1);
        let manifest = world.manifests.load(&id()).await.unwrap();
        assert_eq!(
            manifest.files.get(&rel("mods/sodium.jar")).unwrap(),
            &sha1_hex(b"sodium 2")
        );

        // A second update of the same version downloads nothing.
        let again = update_to(&world, Some(("sodium.jar", b"sodium 2"))).await;
        assert_eq!(again.files.written, 0);
        assert!(world.read_game_file(ID, "mods/sodium.jar").is_none());
    }

    #[tokio::test]
    async fn a_disabled_mod_the_player_turns_back_on_keeps_getting_updates() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        disable(&world, "mods/sodium.jar");
        let (source, downloader) = v1();
        update(&world, &source, &downloader, &CancellationToken::new())
            .await
            .unwrap();
        // Turned back on.
        let off = world.game_file(ID, "mods/sodium.jar.disabled");
        std::fs::rename(&off, world.game_file(ID, "mods/sodium.jar")).unwrap();

        update_to(&world, Some(("sodium.jar", b"sodium 2"))).await;

        assert_eq!(
            world.read_game_file(ID, "mods/sodium.jar").unwrap(),
            b"sodium 2"
        );
    }

    #[tokio::test]
    async fn a_renamed_mod_the_player_disabled_arrives_disabled() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        disable(&world, "mods/sodium.jar");

        update_to(&world, Some(("sodium-2.jar", b"sodium 2"))).await;

        assert!(world.read_game_file(ID, "mods/sodium-2.jar").is_none());
        assert_eq!(
            world
                .read_game_file(ID, "mods/sodium-2.jar.disabled")
                .unwrap(),
            b"sodium 2"
        );
        assert!(
            world
                .read_game_file(ID, "mods/sodium.jar.disabled")
                .is_none(),
            "the old version goes"
        );
        let manifest = world.manifests.load(&id()).await.unwrap();
        assert_eq!(
            manifest.projects.get(&rel("mods/sodium-2.jar")).unwrap(),
            "modrinth:sodium"
        );
    }

    #[tokio::test]
    async fn a_renamed_mod_the_player_left_on_arrives_on() {
        let world = World::new(vec![]);
        install_v1(&world).await;

        update_to(&world, Some(("sodium-2.jar", b"sodium 2"))).await;

        assert_eq!(
            world.read_game_file(ID, "mods/sodium-2.jar").unwrap(),
            b"sodium 2"
        );
        assert!(world.read_game_file(ID, "mods/sodium.jar").is_none());
    }

    #[tokio::test]
    async fn a_turned_off_mod_the_player_changed_is_untracked_until_deleted() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        std::fs::remove_file(world.game_file(ID, "mods/sodium.jar")).unwrap();
        world.write_game_file(ID, "mods/sodium.jar.disabled", b"player build");

        let updated = update_to(&world, Some(("sodium.jar", b"sodium 2"))).await;

        assert_eq!(updated.files.kept, 1);
        assert_eq!(
            world
                .read_game_file(ID, "mods/sodium.jar.disabled")
                .unwrap(),
            b"player build"
        );
        assert!(world.read_game_file(ID, "mods/sodium.jar").is_none());
        let manifest = world.manifests.load(&id()).await.unwrap();
        assert!(!manifest.files.contains_key(&rel("mods/sodium.jar")));

        // Once the player deletes their copy, the pack's version comes back.
        std::fs::remove_file(world.game_file(ID, "mods/sodium.jar.disabled")).unwrap();
        update_to(&world, Some(("sodium.jar", b"sodium 2"))).await;
        assert_eq!(
            world.read_game_file(ID, "mods/sodium.jar").unwrap(),
            b"sodium 2"
        );
    }

    #[tokio::test]
    async fn a_new_pack_mod_never_replaces_the_players_own_turned_off_copy() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        update_to(&world, None).await;
        world.write_game_file(ID, "mods/sodium.jar.disabled", b"player build");

        let updated = update_to(&world, Some(("sodium.jar", b"sodium 2"))).await;

        assert_eq!(updated.files.kept, 1);
        assert!(world.read_game_file(ID, "mods/sodium.jar").is_none());
        assert_eq!(
            world
                .read_game_file(ID, "mods/sodium.jar.disabled")
                .unwrap(),
            b"player build"
        );
    }

    /// Version 1.0 with JEI from CurseForge as file `file_id` named
    /// `filename`, and a catalog that serves it (or lists it without a
    /// download URL when `manual`).
    fn v1_with_jei(
        file_id: u64,
        filename: &str,
        bytes: &[u8],
        manual: bool,
    ) -> (FakePackwizSource, FakeDownloader, FakeCurseForge) {
        let source = source(
            pack_toml(Some("1.0.2"), "1.21.1"),
            vec![
                plain("config/a.toml", b"a1"),
                plain("config/b.toml", b"b1"),
                preserved("config/keep.toml", b"k1"),
                meta("mods/sodium.pw.toml"),
                meta("mods/jei.pw.toml"),
            ],
            vec![
                (
                    "mods/sodium.pw.toml",
                    modrinth("sodium.jar", b"sodium", "both"),
                ),
                (
                    "mods/jei.pw.toml",
                    curseforge_file(file_id, filename, bytes),
                ),
            ],
        );
        let url = format!("https://edge.forgecdn.net/files/{file_id}/{filename}");
        let downloader = FakeDownloader::with([(url.clone(), bytes.to_vec())]);
        let catalog = FakeCurseForge {
            files: vec![CurseForgeFile {
                id: file_id,
                project_id: 238_222,
                file_name: filename.into(),
                length: Some(bytes.len() as u64),
                sha1: Some(sha1_hex(bytes)),
                download_url: (!manual).then_some(url),
            }],
            ..FakeCurseForge::default()
        };
        (source, downloader, catalog)
    }

    async fn update_with_jei(
        world: &World,
        file_id: u64,
        filename: &str,
        bytes: &[u8],
        manual: bool,
    ) -> UpdatedPackwiz {
        let (source, downloader, catalog) = v1_with_jei(file_id, filename, bytes, manual);
        let deps = UpdatePackwizDeps {
            instances: &world.instances,
            source: &source,
            curseforge: &catalog,
            files: file_deps(world, &downloader),
        };
        update_packwiz(
            &id(),
            &deps,
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_renamed_curseforge_mod_the_player_disabled_arrives_disabled() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        update_with_jei(&world, 1, "jei-1.jar", b"jei 1", false).await;
        disable(&world, "mods/jei-1.jar");

        update_with_jei(&world, 2, "jei-2.jar", b"jei 2", false).await;

        assert!(world.read_game_file(ID, "mods/jei-2.jar").is_none());
        assert_eq!(
            world.read_game_file(ID, "mods/jei-2.jar.disabled").unwrap(),
            b"jei 2"
        );
        assert!(
            world
                .read_game_file(ID, "mods/jei-1.jar.disabled")
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_renamed_manual_mod_the_player_disabled_waits_to_arrive_disabled() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        update_with_jei(&world, 1, "jei-1.jar", b"jei 1", false).await;
        disable(&world, "mods/jei-1.jar");

        let updated = update_with_jei(&world, 2, "jei-2.jar", b"jei 2", true).await;

        assert_eq!(updated.pending.len(), 1);
        assert_eq!(updated.pending[0].path, rel("mods/jei-2.jar"));
        assert!(updated.pending[0].disabled, "arrives turned off");
        assert!(
            world
                .read_game_file(ID, "mods/jei-1.jar.disabled")
                .is_none()
        );

        // Still pending on the next sync, still to arrive turned off.
        let again = update_with_jei(&world, 2, "jei-2.jar", b"jei 2", true).await;
        assert!(again.pending[0].disabled);
        // Renamed again while still pending: the mark follows the mod.
        let renamed = update_with_jei(&world, 3, "jei-3.jar", b"jei 3", true).await;
        assert_eq!(renamed.pending[0].path, rel("mods/jei-3.jar"));
        assert!(renamed.pending[0].disabled);
    }

    #[tokio::test]
    async fn an_intact_disabled_mod_the_pack_drops_is_removed() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        disable(&world, "mods/sodium.jar");

        let updated = update_to(&world, None).await;

        assert!(world.read_game_file(ID, "mods/sodium.jar").is_none());
        assert!(
            world
                .read_game_file(ID, "mods/sodium.jar.disabled")
                .is_none()
        );
        assert_eq!(updated.files.removed, 1);
    }

    #[tokio::test]
    async fn a_changed_disabled_mod_the_pack_drops_is_kept() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        std::fs::remove_file(world.game_file(ID, "mods/sodium.jar")).unwrap();
        world.write_game_file(ID, "mods/sodium.jar.disabled", b"player build");

        let updated = update_to(&world, None).await;

        assert_eq!(
            world
                .read_game_file(ID, "mods/sodium.jar.disabled")
                .unwrap(),
            b"player build"
        );
        assert_eq!(updated.files.removed, 0);
    }

    #[tokio::test]
    async fn a_hand_downloaded_file_the_player_turned_off_stays() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        let (source, downloader) = v2();
        let cancel = CancellationToken::new();
        let sync = || update(&world, &source, &downloader, &cancel);
        sync().await.unwrap();
        // JEI is a manual CurseForge download.
        world.write_game_file(ID, "mods/jei.jar", b"jei");
        assert!(sync().await.unwrap().pending.is_empty());
        disable(&world, "mods/jei.jar");

        let updated = sync().await.unwrap();

        assert_eq!(
            world.read_game_file(ID, "mods/jei.jar.disabled").unwrap(),
            b"jei"
        );
        assert!(world.read_game_file(ID, "mods/jei.jar").is_none());
        assert!(updated.pending.is_empty());
        assert_eq!(updated.files.removed, 0);
    }

    #[tokio::test]
    async fn a_pack_mod_the_player_deleted_comes_back() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        std::fs::remove_file(world.game_file(ID, "mods/sodium.jar")).unwrap();

        let (source, downloader) = v1();
        update(&world, &source, &downloader, &CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(
            world.read_game_file(ID, "mods/sodium.jar").unwrap(),
            b"sodium"
        );
    }

    #[tokio::test]
    async fn an_unchanged_pack_downloads_nothing() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        let (source, downloader) = v1();
        let updated = update(&world, &source, &downloader, &CancellationToken::new())
            .await
            .unwrap();
        assert!(downloader.requests().is_empty());
        assert_eq!(updated.files.written + updated.files.removed, 0);
        assert_eq!(updated.files.unchanged, 4);
    }

    #[tokio::test]
    async fn refuses_an_instance_not_installed_from_packwiz() {
        let world = World::new(vec![]);
        create_instance(
            NewInstance {
                name: "Cinepol Pack",
                game_version: "1.21.1",
                loader: LoaderSpec::vanilla(),
                origin: InstanceOrigin::Custom,
                seed_server: None,
            },
            &world.instances,
            &world.servers,
        )
        .await
        .unwrap();
        let (source, downloader) = v2();
        assert_eq!(
            update(&world, &source, &downloader, &CancellationToken::new()).await,
            Err(UpdatePackwizError::NotPackwiz(id()))
        );
        assert!(downloader.requests().is_empty());
    }

    #[tokio::test]
    async fn a_failed_sync_keeps_the_previous_manifest_and_instance() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        let manifest = world.manifests.load(&id()).await.unwrap();
        let before = world.instances.get(&id()).await.unwrap();
        let (source, _) = v2();
        let result = update(
            &world,
            &source,
            &FailingDownloader,
            &CancellationToken::new(),
        )
        .await;
        assert!(matches!(
            result,
            Err(UpdatePackwizError::Pack(PackSyncError::Download(_)))
        ));
        assert_eq!(world.manifests.load(&id()).await.unwrap(), manifest);
        assert_eq!(world.instances.get(&id()).await.unwrap(), before);
        assert_eq!(world.read_game_file(ID, "config/b.toml").unwrap(), b"b1");
    }

    #[tokio::test]
    async fn cancellation_stops_before_downloading() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        let (source, downloader) = v2();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = update(&world, &source, &downloader, &cancel).await;
        assert_eq!(result, Err(UpdatePackwizError::Cancelled));
        assert!(downloader.requests().is_empty());
    }

    /// Renames the instance while its files download.
    struct EditingDownloader<'a> {
        inner: FakeDownloader,
        instances: &'a crate::testing::InMemoryInstanceRepository,
    }

    impl Downloader for EditingDownloader<'_> {
        async fn download_all<R: ProgressReporter>(
            &self,
            requests: Vec<crate::domain::ports::DownloadRequest>,
            reporter: &R,
            cancel: &CancellationToken,
        ) -> Result<crate::domain::ports::DownloadSummary, crate::domain::ports::DownloadError>
        {
            let mut instance = self.instances.get(&id()).await.unwrap();
            instance.name = crate::domain::instance::InstanceName::parse("Renamed").unwrap();
            self.instances.save(&instance).await.unwrap();
            self.inner.download_all(requests, reporter, cancel).await
        }
    }

    #[tokio::test]
    async fn a_settings_edit_saved_during_an_update_is_kept() {
        let world = World::new(vec![]);
        install_v1(&world).await;
        let (source, inner) = v2();
        let downloader = EditingDownloader {
            inner,
            instances: &world.instances,
        };
        update(&world, &source, &downloader, &CancellationToken::new())
            .await
            .unwrap();
        let stored = world.instances.get(&id()).await.unwrap();
        assert_eq!(stored.name.as_str(), "Renamed");
        assert_eq!(stored.game_version.as_str(), "1.21.2");
    }
}
