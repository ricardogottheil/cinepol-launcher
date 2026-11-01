//! "Play on CINEPOL": prepare the managed instance, install what is missing
//! and launch straight into a community server (the main one unless the
//! player picked another), or into the game when the config lists none.
//!
//! When the config names a community pack, the managed instance is the
//! packwiz instance installed from that pack: installed when missing and
//! synced before every launch. Without a pack it is the Vanilla instance.

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use std::sync::OnceLock;

use crate::application::create_instance::CreateInstanceError;
use crate::application::install_instance::{InstallDeps, InstallError, install_instance};
use crate::application::install_packwiz::{
    InstallPackwizDeps, InstallPackwizError, InstallPackwizRequest, install_packwiz,
};
use crate::application::launch_instance::{
    LaunchDeps, LaunchError, LaunchOutcome, LaunchRequest, launch_instance,
};
use crate::application::modpack_files::FileSyncDeps;
use crate::application::prepare_cinepol_instance::{
    cinepol_server_entry, find_cinepol_instance, find_cinepol_pack_instance,
    prepare_cinepol_instance,
};
use crate::application::refresh_session::PremiumSessions;
use crate::application::update_packwiz::{UpdatePackwizDeps, UpdatePackwizError, update_packwiz};
use crate::domain::account::AccountRepository;
use crate::domain::instance::{Instance, InstanceId, InstanceRepository};
use crate::domain::java::JavaRuntimeProvider;
use crate::domain::launch::{
    GameProcessRunner, LogSink, NativesExtractor, RunningGames, VersionSource,
};
use crate::domain::loader::LoaderInstaller;
use crate::domain::modpack::{
    CurseForgeLookup, PackFiles, PackLocation, PackManifestStore, PackwizSource,
};
use crate::domain::ports::{
    Clock, Downloader, FileSystem, ProgressReporter, ProgressStage, ProgressUpdate,
};
use crate::domain::server::ServerConfig;
use crate::domain::server_list::{ServerAddress, ServerListStore};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlayOnCinepolError {
    #[error(transparent)]
    Prepare(#[from] CreateInstanceError),
    #[error(transparent)]
    Install(#[from] InstallError),
    #[error(transparent)]
    Launch(#[from] LaunchError),
    #[error("{0} is already running")]
    AlreadyRunning(InstanceId),
    #[error("{0} is not a CINEPOL server")]
    UnknownServer(String),
    #[error(transparent)]
    InstallPack(InstallPackwizError),
    /// Only a cancelled sync stops the session; any other sync failure
    /// launches the pack as it is installed.
    #[error(transparent)]
    UpdatePack(UpdatePackwizError),
}

/// What Play needs to install or sync the community pack.
pub struct CinepolPackDeps<'a, W, K, PD, PF, PP, PM> {
    pub source: &'a W,
    /// Resolves CurseForge-mode files; without a key they stay manual.
    pub curseforge: &'a K,
    pub files: FileSyncDeps<'a, PD, PF, PP, PM>,
}

impl<W, K, PD, PF, PP, PM> CinepolPackDeps<'_, W, K, PD, PF, PP, PM> {
    fn files(&self) -> FileSyncDeps<'_, PD, PF, PP, PM> {
        FileSyncDeps {
            downloader: self.files.downloader,
            fs: self.files.fs,
            files: self.files.files,
            manifests: self.files.manifests,
            paths: self.files.paths,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CinepolSession {
    pub instance: Instance,
    pub outcome: LaunchOutcome,
}

/// Reports a `Launching` step between install and launch so the UI can show
/// the game as starting. `cancel` stops the install; `kill` stops the game.
///
/// The instance is claimed in `games` (with `kill`) before it is changed, so a
/// session never moves or reinstalls an instance that is already running, and
/// stopping the instance from elsewhere stops this session.
#[allow(clippy::too_many_arguments)]
pub async fn play_on_cinepol<I, A, V, D, J, O, N, P, C, F, Q, S, G, R, L, W, K, PD, PF, PP, PM>(
    config: &ServerConfig,
    server: Option<&ServerAddress>,
    servers: &S,
    games: &G,
    install: &InstallDeps<'_, I, V, D, J, O>,
    launch: &LaunchDeps<'_, I, A, V, J, N, P, C, F, Q>,
    pack: &CinepolPackDeps<'_, W, K, PD, PF, PP, PM>,
    reporter: &R,
    logs: L,
    cancel: &CancellationToken,
    kill: CancellationToken,
) -> Result<CinepolSession, PlayOnCinepolError>
where
    I: InstanceRepository,
    A: AccountRepository,
    V: VersionSource,
    D: Downloader,
    J: JavaRuntimeProvider,
    O: LoaderInstaller,
    N: NativesExtractor,
    P: GameProcessRunner,
    C: Clock,
    F: FileSystem,
    Q: PremiumSessions,
    S: ServerListStore,
    G: RunningGames,
    R: ProgressReporter,
    L: LogSink,
    W: PackwizSource,
    K: CurseForgeLookup,
    PD: Downloader,
    PF: FileSystem,
    PP: PackFiles,
    PM: PackManifestStore,
{
    let join = match server {
        Some(address) => Some(
            config
                .server(address)
                .ok_or_else(|| PlayOnCinepolError::UnknownServer(address.to_string()))?,
        ),
        None => config.main_server(),
    }
    .map(|server| server.address.to_string());
    let location = config
        .pack
        .as_ref()
        .map(|url| PackLocation::Url(url.as_str().to_owned()));
    // Set when the pack instance already existed and must be synced.
    let mut sync = None;
    let id = match &location {
        Some(location) => {
            let (id, installed_now) = claim_pack_instance(
                config, location, servers, games, install, pack, reporter, cancel, &kill,
            )
            .await?;
            if !installed_now {
                sync = Some(location);
            }
            id
        }
        None => {
            let id = match find_cinepol_instance(install.instances)
                .await
                .map_err(CreateInstanceError::from)?
            {
                Some(instance) => instance.id,
                None => {
                    prepare_cinepol_instance(config, install.instances, servers)
                        .await?
                        .id
                }
            };
            if !games.claim(&id, &kill) {
                return Err(PlayOnCinepolError::AlreadyRunning(id));
            }
            id
        }
    };
    let _claim = Claim { games, id: &id };
    async {
        let instance = match &location {
            Some(_) => install
                .instances
                .get(&id)
                .await
                .map_err(CreateInstanceError::from)?,
            None => prepare_cinepol_instance(config, install.instances, servers).await?,
        };
        if let Some(location) = sync {
            sync_pack(&instance.id, location, install, pack, reporter, cancel).await?;
        }
        install_instance(&instance.id, install, reporter, cancel).await?;
        reporter.report(ProgressUpdate {
            stage: Some(ProgressStage::Launching),
            ..ProgressUpdate::new(0, Some(1))
        });
        let request = LaunchRequest {
            instance: instance.id.clone(),
            account: None,
            join_server: join,
        };
        let outcome = launch_instance(&request, launch, logs, kill).await?;
        Ok(CinepolSession { instance, outcome })
    }
    .await
}

/// The claimed community pack instance: the one installed from `location`,
/// or a new one installed now (named after the community and seeded with its
/// main server; `true`). A failed install releases the claim again.
#[allow(clippy::too_many_arguments)]
async fn claim_pack_instance<I, V, D, J, O, S, G, R, W, K, PD, PF, PP, PM>(
    config: &ServerConfig,
    location: &PackLocation,
    servers: &S,
    games: &G,
    install: &InstallDeps<'_, I, V, D, J, O>,
    pack: &CinepolPackDeps<'_, W, K, PD, PF, PP, PM>,
    reporter: &R,
    cancel: &CancellationToken,
    kill: &CancellationToken,
) -> Result<(InstanceId, bool), PlayOnCinepolError>
where
    I: InstanceRepository,
    S: ServerListStore,
    G: RunningGames,
    R: ProgressReporter,
    W: PackwizSource,
    K: CurseForgeLookup,
    PD: Downloader,
    PF: FileSystem,
    PP: PackFiles,
    PM: PackManifestStore,
{
    let existing = find_cinepol_pack_instance(install.instances, location)
        .await
        .map_err(CreateInstanceError::from)?;
    if let Some(instance) = existing {
        if !games.claim(&instance.id, kill) {
            return Err(PlayOnCinepolError::AlreadyRunning(instance.id));
        }
        return Ok((instance.id, false));
    }
    let claimed = OnceLock::new();
    let claim = |id: &InstanceId| {
        let free = games.claim(id, kill);
        if free {
            let _ = claimed.set(id.clone());
        }
        free
    };
    let installed = install_packwiz(
        InstallPackwizRequest {
            pack: location,
            name: Some(&config.name),
            seed_server: cinepol_server_entry(config),
            claim: Some(&claim),
        },
        &InstallPackwizDeps {
            instances: install.instances,
            servers,
            source: pack.source,
            curseforge: pack.curseforge,
            files: pack.files(),
        },
        reporter,
        cancel,
    )
    .await;
    match installed {
        Ok(installed) => Ok((installed.instance.id, true)),
        Err(error) => {
            if let Some(id) = claimed.get() {
                games.release(id);
            }
            Err(PlayOnCinepolError::InstallPack(error))
        }
    }
}

/// Brings the pack instance to the pack's current version. A cancelled sync
/// ends the session; any other failure is logged and the installed pack is
/// launched, so an unreachable pack host never keeps the player out.
async fn sync_pack<I, V, D, J, O, R, W, K, PD, PF, PP, PM>(
    id: &InstanceId,
    location: &PackLocation,
    install: &InstallDeps<'_, I, V, D, J, O>,
    pack: &CinepolPackDeps<'_, W, K, PD, PF, PP, PM>,
    reporter: &R,
    cancel: &CancellationToken,
) -> Result<(), PlayOnCinepolError>
where
    I: InstanceRepository,
    R: ProgressReporter,
    W: PackwizSource,
    K: CurseForgeLookup,
    PD: Downloader,
    PF: FileSystem,
    PP: PackFiles,
    PM: PackManifestStore,
{
    let deps = UpdatePackwizDeps {
        instances: install.instances,
        source: pack.source,
        curseforge: pack.curseforge,
        files: pack.files(),
    };
    match update_packwiz(id, &deps, reporter, cancel).await {
        Ok(_) => Ok(()),
        Err(UpdatePackwizError::Cancelled) => Err(PlayOnCinepolError::UpdatePack(
            UpdatePackwizError::Cancelled,
        )),
        Err(error) => {
            tracing::warn!(%error, pack = %location.as_string(), "could not sync the CINEPOL pack; launching the installed one");
            Ok(())
        }
    }
}

/// Releases a claimed instance however the session ends, including when its
/// future is dropped before it finishes.
struct Claim<'a, G: RunningGames> {
    games: &'a G,
    id: &'a InstanceId,
}

impl<G: RunningGames> Drop for Claim<'_, G> {
    fn drop(&mut self) {
        self.games.release(self.id);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::application::install_packwiz::fixtures::{
        BASE, cdn, location, meta, modrinth, pack_toml, plain, source,
    };
    use crate::application::load_server_config::fixtures::cinepol;
    use crate::application::modpack_files::fixtures::FailingDownloader;
    use crate::domain::account::{Account, AccountId, Username};
    use crate::domain::instance::{GameVersion, InstanceName, InstanceOrigin};
    use crate::domain::java::{JavaRuntime, JavaSource, JavaVersion};
    use crate::domain::launch::{Arch, LauncherPaths, OsName, Platform, VersionManifest};
    use crate::domain::loader::LoaderSpec;
    use crate::domain::server::{CommunityServer, HttpsUrl};
    use crate::testing::{
        FakeDownloader, FakeFileSystem, FakeGameProcess, FakeJavaRuntimes, FakeVersionSource,
        FixedClock, InMemoryAccountRepository, InMemoryInstanceRepository, InMemoryRunningGames,
        InMemoryServerListStore, RecordingLogSink, RecordingNativesExtractor, RecordingReporter,
        sample_asset_index, sample_version_spec,
    };
    use crate::testing::{FakePackFiles, FakePackwizSource, InMemoryPackManifestStore};

    /// Every dependency of a session, with an active offline account.
    struct World {
        _dir: tempfile::TempDir,
        instances: InMemoryInstanceRepository,
        servers: InMemoryServerListStore,
        games: InMemoryRunningGames,
        accounts: InMemoryAccountRepository,
        versions: FakeVersionSource,
        java: FakeJavaRuntimes,
        paths: LauncherPaths,
        fs: FakeFileSystem,
        downloader: FakeDownloader,
        natives: RecordingNativesExtractor,
        runner: FakeGameProcess,
        clock: FixedClock,
        platform: Platform,
        reporter: RecordingReporter,
        pack_files: FakePackFiles,
        manifests: InMemoryPackManifestStore,
    }

    /// The community pack, version 1.0: one config file and Sodium.
    fn pack_v1() -> (FakePackwizSource, FakeDownloader) {
        pack_with(
            "1.0",
            vec![plain("config/a.toml", b"a1"), meta("mods/sodium.pw.toml")],
            [(format!("{BASE}config/a.toml"), b"a1".to_vec())],
        )
    }

    /// Version 1.1: the config file changed.
    fn pack_v2() -> (FakePackwizSource, FakeDownloader) {
        pack_with(
            "1.1",
            vec![plain("config/a.toml", b"a2"), meta("mods/sodium.pw.toml")],
            [(format!("{BASE}config/a.toml"), b"a2".to_vec())],
        )
    }

    fn pack_with(
        version: &str,
        files: Vec<crate::domain::modpack::packwiz::RawIndexEntry>,
        bodies: [(String, Vec<u8>); 1],
    ) -> (FakePackwizSource, FakeDownloader) {
        let source = source(
            pack_toml(Some(version), "26.2"),
            files,
            vec![(
                "mods/sodium.pw.toml",
                modrinth("sodium.jar", b"sodium", "both"),
            )],
        );
        let mut all = bodies.to_vec();
        all.push((cdn("sodium.jar"), b"sodium".to_vec()));
        (source, FakeDownloader::with(all))
    }

    /// The default config plus the community pack at the fixtures' location.
    fn with_pack(mut config: ServerConfig) -> ServerConfig {
        config.pack = Some(HttpsUrl::parse(&location().as_string()).unwrap());
        config
    }

    fn pack_version(instance: &Instance) -> Option<String> {
        match &instance.origin {
            InstanceOrigin::Modpack(pack) => Some(pack.version.clone()),
            _ => None,
        }
    }

    impl World {
        async fn new() -> Self {
            let account = Account::offline(
                AccountId::new_random(),
                Username::parse("polispol1").unwrap(),
            );
            let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
            accounts.set_active(Some(account.id)).await.unwrap();
            // The fake downloader writes real files, so the data root is a temp dir.
            let dir = tempfile::tempdir().unwrap();
            let paths = LauncherPaths::new(dir.path());
            let fs = FakeFileSystem::with([
                paths
                    .libraries_dir()
                    .join("org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1.jar"),
                paths.versions_dir().join("26.2/26.2.jar"),
                paths.assets_dir().join("log_configs/client-1.21.2.xml"),
                paths.assets_dir().join("indexes/32.json"),
            ]);
            Self {
                _dir: dir,
                instances: InMemoryInstanceRepository::default(),
                servers: InMemoryServerListStore::default(),
                games: InMemoryRunningGames::default(),
                accounts,
                versions: FakeVersionSource {
                    manifest: VersionManifest {
                        latest_release: GameVersion::parse("26.2").unwrap(),
                        latest_snapshot: GameVersion::parse("26.2").unwrap(),
                        versions: vec![],
                    },
                    versions: HashMap::from([
                        ("26.2".to_owned(), sample_version_spec("26.2")),
                        // The Fabric profile the community pack fixture asks for.
                        ("fabric-loader-0.16.5-26.2".to_owned(), {
                            let mut profile = sample_version_spec("26.2");
                            profile.id = "fabric-loader-0.16.5-26.2".into();
                            profile
                        }),
                    ]),
                    asset_indexes: HashMap::from([("32".to_owned(), sample_asset_index())]),
                },
                java: FakeJavaRuntimes::providing(JavaRuntime {
                    executable: PathBuf::from("/data/runtimes/java"),
                    version: JavaVersion {
                        major: 25,
                        full: "25.0.1".into(),
                        is_64_bit: true,
                    },
                    source: JavaSource::Managed,
                }),
                paths,
                fs,
                downloader: FakeDownloader::default(),
                natives: RecordingNativesExtractor::default(),
                runner: FakeGameProcess::exiting_with(0),
                clock: FixedClock::at_unix(1_000),
                platform: Platform::new(OsName::MacOs, Arch::Arm64),
                reporter: RecordingReporter::default(),
                pack_files: FakePackFiles::default(),
                manifests: InMemoryPackManifestStore::default(),
            }
        }

        async fn play(
            &self,
            kill: CancellationToken,
        ) -> Result<CinepolSession, PlayOnCinepolError> {
            self.play_on(&cinepol("26.2"), None, kill).await
        }

        async fn play_on(
            &self,
            config: &ServerConfig,
            server: Option<&ServerAddress>,
            kill: CancellationToken,
        ) -> Result<CinepolSession, PlayOnCinepolError> {
            let (source, downloader) = pack_v1();
            self.play_with_pack(config, server, &source, &downloader, kill)
                .await
        }

        async fn play_with_pack<PD: Downloader>(
            &self,
            config: &ServerConfig,
            server: Option<&ServerAddress>,
            source: &FakePackwizSource,
            pack_downloader: &PD,
            kill: CancellationToken,
        ) -> Result<CinepolSession, PlayOnCinepolError> {
            let install = InstallDeps {
                instances: &self.instances,
                versions: &self.versions,
                downloader: &self.downloader,
                java: &self.java,
                paths: &self.paths,
                platform: &self.platform,
                loaders: &crate::testing::FakeLoaderInstaller::default(),
            };
            let launch = LaunchDeps {
                instances: &self.instances,
                accounts: &self.accounts,
                versions: &self.versions,
                java: &self.java,
                natives: &self.natives,
                runner: &self.runner,
                clock: &self.clock,
                fs: &self.fs,
                paths: &self.paths,
                platform: &self.platform,
                sessions: &crate::application::refresh_session::NoPremiumSessions,
                launcher_name: "cinepol-launcher",
                launcher_version: "0.1.0",
            };
            let pack = CinepolPackDeps {
                source,
                curseforge: crate::testing::no_curseforge(),
                files: FileSyncDeps {
                    downloader: pack_downloader,
                    fs: &self.fs,
                    files: &self.pack_files,
                    manifests: &self.manifests,
                    paths: &self.paths,
                },
            };
            play_on_cinepol(
                config,
                server,
                &self.servers,
                &self.games,
                &install,
                &launch,
                &pack,
                &self.reporter,
                RecordingLogSink::default(),
                &CancellationToken::new(),
                kill,
            )
            .await
        }
    }

    #[tokio::test]
    async fn prepares_installs_and_joins_the_server_with_quick_play() {
        let world = World::new().await;

        let session = world.play(CancellationToken::new()).await.unwrap();

        assert_eq!(session.instance.id.as_str(), "cinepol");
        assert!(session.outcome.exit.success());
        assert!(
            !world.downloader.requests().is_empty(),
            "installed before launching"
        );
        let args = world.runner.specs()[0].command.args();
        assert_eq!(
            &args[args.len() - 2..],
            ["--quickPlayMultiplayer", "mc.policarpiers.com"]
        );
        assert_eq!(
            world.reporter.updates().last().and_then(|u| u.stage),
            Some(ProgressStage::Launching)
        );
        assert_eq!(
            world
                .servers
                .load(&session.instance.id)
                .await
                .unwrap()
                .entries()
                .len(),
            1
        );
        assert!(
            !world.games.is_busy(&session.instance.id),
            "released after the game exits"
        );
    }

    fn with_creative(mut config: ServerConfig) -> ServerConfig {
        config.servers.push(CommunityServer {
            name: "Creative".into(),
            address: ServerAddress::parse("creative.policarpiers.com").unwrap(),
        });
        config
    }

    fn joined(world: &World) -> Option<String> {
        let args = world.runner.specs()[0].command.args().to_vec();
        args.iter()
            .position(|arg| arg == "--quickPlayMultiplayer")
            .map(|index| args[index + 1].clone())
    }

    #[tokio::test]
    async fn joins_the_community_server_the_player_picked() {
        let world = World::new().await;
        let creative = ServerAddress::parse("creative.policarpiers.com").unwrap();
        world
            .play_on(
                &with_creative(cinepol("26.2")),
                Some(&creative),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(joined(&world).as_deref(), Some("creative.policarpiers.com"));
    }

    #[tokio::test]
    async fn refuses_a_server_the_config_does_not_list_before_installing() {
        let world = World::new().await;
        let elsewhere = ServerAddress::parse("mc.hypixel.net").unwrap();
        let result = world
            .play_on(&cinepol("26.2"), Some(&elsewhere), CancellationToken::new())
            .await;
        assert_eq!(
            result,
            Err(PlayOnCinepolError::UnknownServer("mc.hypixel.net".into()))
        );
        assert!(world.downloader.requests().is_empty(), "nothing installed");
        assert!(world.runner.specs().is_empty(), "no game started");
    }

    #[tokio::test]
    async fn starts_the_game_without_joining_when_the_config_lists_no_server() {
        let world = World::new().await;
        let mut config = cinepol("26.2");
        config.servers.clear();
        let session = world
            .play_on(&config, None, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(joined(&world), None);
        assert_eq!(session.instance.name.as_str(), "CINEPOL");
        assert!(
            world
                .servers
                .load(&session.instance.id)
                .await
                .unwrap()
                .entries()
                .is_empty(),
            "no server to seed"
        );
    }

    #[tokio::test]
    async fn refuses_an_instance_that_is_already_running_without_touching_it() {
        let world = World::new().await;
        let id = InstanceId::parse("cinepol").unwrap();
        let running = Instance::new(
            id.clone(),
            InstanceName::parse("CINEPOL").unwrap(),
            GameVersion::parse("1.21.11").unwrap(),
            LoaderSpec::vanilla(),
            InstanceOrigin::CinepolServer,
        );
        world.instances.save(&running).await.unwrap();
        assert!(world.games.claim(&id, &CancellationToken::new()));

        let result = world.play(CancellationToken::new()).await;

        assert_eq!(result, Err(PlayOnCinepolError::AlreadyRunning(id.clone())));
        assert_eq!(
            world.instances.list().await.unwrap(),
            [running],
            "the running instance keeps its version"
        );
        assert!(world.downloader.requests().is_empty(), "nothing installed");
        assert!(world.runner.specs().is_empty(), "no second game process");
        assert!(
            world.games.is_busy(&id),
            "the other session keeps its claim"
        );
    }

    #[tokio::test]
    async fn claims_the_instance_with_the_kill_signal_and_releases_it_on_failure() {
        let world = World::new().await;
        let kill = CancellationToken::new();
        // No account: the launch fails after the instance was claimed.
        world.accounts.set_active(None).await.unwrap();

        let result = world.play(kill.clone()).await;

        assert!(matches!(result, Err(PlayOnCinepolError::Launch(_))));
        let id = InstanceId::parse("cinepol").unwrap();
        assert!(!world.games.is_busy(&id), "released after a failed session");
        assert!(world.games.claim(&id, &kill), "free for the next session");
    }

    #[tokio::test]
    async fn a_dropped_session_releases_its_claim() {
        let mut world = World::new().await;
        world.runner = FakeGameProcess::running_until_killed();

        let dropped = tokio::time::timeout(
            Duration::from_millis(100),
            world.play(CancellationToken::new()),
        )
        .await;

        assert!(dropped.is_err(), "the game was still running");
        let id = InstanceId::parse("cinepol").unwrap();
        assert!(
            !world.games.is_busy(&id),
            "released when the future is dropped"
        );
    }

    #[tokio::test]
    async fn with_a_pack_and_no_instance_installs_the_pack_and_joins_the_server() {
        let world = World::new().await;
        let config = with_pack(cinepol("26.2"));

        let session = world
            .play_on(&config, None, CancellationToken::new())
            .await
            .unwrap();

        let instance = &session.instance;
        assert_eq!(instance.name.as_str(), "CINEPOL");
        assert_eq!(
            instance.origin,
            InstanceOrigin::Modpack(crate::domain::modpack::ModpackRef {
                source: crate::domain::modpack::ModpackSourceKind::Packwiz,
                project: location().as_string(),
                version: "1.0".into(),
            })
        );
        assert_ne!(
            instance.loader,
            LoaderSpec::vanilla(),
            "the pack's Fabric loader"
        );
        assert_eq!(world.instances.list().await.unwrap().len(), 1);
        assert_eq!(
            world
                .servers
                .load(&instance.id)
                .await
                .unwrap()
                .entries()
                .len(),
            1,
            "seeded with the main server"
        );
        let args = world.runner.specs()[0].command.args();
        assert_eq!(
            &args[args.len() - 2..],
            ["--quickPlayMultiplayer", "mc.policarpiers.com"]
        );
        assert!(!world.games.is_busy(&instance.id));
    }

    #[tokio::test]
    async fn with_a_pack_the_installed_pack_instance_is_synced_and_reused() {
        let world = World::new().await;
        let config = with_pack(cinepol("26.2"));
        let first = world
            .play_on(&config, None, CancellationToken::new())
            .await
            .unwrap();

        let (source, downloader) = pack_v2();
        let second = world
            .play_with_pack(
                &config,
                None,
                &source,
                &downloader,
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(second.instance.id, first.instance.id);
        assert_eq!(world.instances.list().await.unwrap().len(), 1);
        let stored = world.instances.get(&first.instance.id).await.unwrap();
        assert_eq!(pack_version(&stored).as_deref(), Some("1.1"));
        assert_eq!(world.runner.specs().len(), 2, "launched both times");
    }

    #[tokio::test]
    async fn an_unreachable_pack_still_launches_the_installed_one() {
        let world = World::new().await;
        let config = with_pack(cinepol("26.2"));
        let first = world
            .play_on(&config, None, CancellationToken::new())
            .await
            .unwrap();

        let (source, _) = pack_v2();
        let session = world
            .play_with_pack(
                &config,
                None,
                &source,
                &FailingDownloader,
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(session.instance.id, first.instance.id);
        let stored = world.instances.get(&first.instance.id).await.unwrap();
        assert_eq!(
            pack_version(&stored).as_deref(),
            Some("1.0"),
            "kept as installed"
        );
        assert_eq!(world.runner.specs().len(), 2);
    }

    #[tokio::test]
    async fn a_failed_pack_install_leaves_no_instance_and_releases_the_claim() {
        let world = World::new().await;
        let config = with_pack(cinepol("26.2"));
        let (source, _) = pack_v1();

        let result = world
            .play_with_pack(
                &config,
                None,
                &source,
                &FailingDownloader,
                CancellationToken::new(),
            )
            .await;

        assert!(
            matches!(result, Err(PlayOnCinepolError::InstallPack(_))),
            "{result:?}"
        );
        assert!(world.instances.list().await.unwrap().is_empty());
        assert!(world.runner.specs().is_empty());
        let retry = world.play_on(&config, None, CancellationToken::new()).await;
        assert!(retry.is_ok(), "nothing stays claimed: {retry:?}");
    }

    #[tokio::test]
    async fn with_a_pack_an_old_vanilla_cinepol_instance_is_left_alone() {
        let world = World::new().await;
        let vanilla = world.play(CancellationToken::new()).await.unwrap().instance;

        let session = world
            .play_on(&with_pack(cinepol("26.2")), None, CancellationToken::new())
            .await
            .unwrap();

        assert_ne!(session.instance.id, vanilla.id);
        assert_eq!(world.instances.list().await.unwrap().len(), 2);
        assert_eq!(world.instances.get(&vanilla.id).await.unwrap(), vanilla);
    }
}
