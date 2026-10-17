//! "Play on CINEPOL": prepare the managed instance, install what is missing
//! and launch straight into the server.

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::create_instance::CreateInstanceError;
use crate::application::install_instance::{InstallDeps, InstallError, install_instance};
use crate::application::launch_instance::{
    LaunchDeps, LaunchError, LaunchOutcome, LaunchRequest, launch_instance,
};
use crate::application::prepare_cinepol_instance::{
    find_cinepol_instance, prepare_cinepol_instance,
};
use crate::domain::account::AccountRepository;
use crate::domain::instance::{Instance, InstanceId, InstanceRepository};
use crate::domain::java::JavaRuntimeProvider;
use crate::domain::launch::{
    GameProcessRunner, LogSink, NativesExtractor, RunningGames, VersionSource,
};
use crate::domain::ports::{
    Clock, Downloader, FileSystem, ProgressReporter, ProgressStage, ProgressUpdate,
};
use crate::domain::server::ServerConfig;
use crate::domain::server_list::ServerListStore;

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
pub async fn play_on_cinepol<I, A, V, D, J, N, P, C, F, S, G, R, L>(
    config: &ServerConfig,
    servers: &S,
    games: &G,
    install: &InstallDeps<'_, I, V, D, J>,
    launch: &LaunchDeps<'_, I, A, V, J, N, P, C, F>,
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
    N: NativesExtractor,
    P: GameProcessRunner,
    C: Clock,
    F: FileSystem,
    S: ServerListStore,
    G: RunningGames,
    R: ProgressReporter,
    L: LogSink,
{
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
    let session = async {
        let instance = prepare_cinepol_instance(config, install.instances, servers).await?;
        install_instance(&instance.id, install, reporter, cancel).await?;
        reporter.report(ProgressUpdate {
            stage: Some(ProgressStage::Launching),
            ..ProgressUpdate::new(0, Some(1))
        });
        let request = LaunchRequest {
            instance: instance.id.clone(),
            account: None,
            join_server: Some(config.address.to_string()),
        };
        let outcome = launch_instance(&request, launch, logs, kill).await?;
        Ok(CinepolSession { instance, outcome })
    }
    .await;
    games.release(&id);
    session
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;
    use crate::application::load_server_config::fixtures::cinepol;
    use crate::domain::account::{Account, AccountId, Username};
    use crate::domain::instance::{GameVersion, InstanceName, InstanceOrigin};
    use crate::domain::java::{JavaRuntime, JavaSource, JavaVersion};
    use crate::domain::launch::{Arch, LauncherPaths, OsName, Platform, VersionManifest};
    use crate::domain::loader::LoaderSpec;
    use crate::testing::{
        FakeDownloader, FakeFileSystem, FakeGameProcess, FakeJavaRuntimes, FakeVersionSource,
        FixedClock, InMemoryAccountRepository, InMemoryInstanceRepository, InMemoryRunningGames,
        InMemoryServerListStore, RecordingLogSink, RecordingNativesExtractor, RecordingReporter,
        sample_asset_index, sample_version_spec,
    };

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
                    versions: HashMap::from([("26.2".to_owned(), sample_version_spec("26.2"))]),
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
            }
        }

        async fn play(
            &self,
            kill: CancellationToken,
        ) -> Result<CinepolSession, PlayOnCinepolError> {
            let install = InstallDeps {
                instances: &self.instances,
                versions: &self.versions,
                downloader: &self.downloader,
                java: &self.java,
                paths: &self.paths,
                platform: &self.platform,
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
                launcher_name: "cinepol-launcher",
                launcher_version: "0.1.0",
            };
            play_on_cinepol(
                &cinepol("26.2"),
                &self.servers,
                &self.games,
                &install,
                &launch,
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
}
