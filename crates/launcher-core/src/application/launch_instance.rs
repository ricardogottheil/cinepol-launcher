//! Starts an installed instance with an account and waits for the game to exit.

use crate::domain::loader::{LoaderKind, profile_id};
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::refresh_session::{PremiumSessions, SessionError};
use crate::domain::account::{AccountId, AccountKind, AccountRepository, AccountRepositoryError};
use crate::domain::instance::{
    InstanceId, InstanceRepository, InstanceRepositoryError, JavaSelection,
};
use crate::domain::java::{JavaRequirement, JavaRuntimeError, JavaRuntimeProvider};
use crate::domain::launch::{
    ExtractError, Features, GameExit, GameProcessError, GameProcessRunner, LaunchContext,
    LaunchIdentity, LauncherPaths, LogSink, NativesExtractor, Platform, ProcessSpec, VersionSource,
    VersionSourceError, build_launch_command, plan_libraries,
};
use crate::domain::ports::{Clock, FileSystem, FileSystemError, ProgressReporter, ProgressUpdate};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LaunchError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Accounts(#[from] AccountRepositoryError),
    #[error("no account to play with; add one first")]
    NoAccount,
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error("{0:?} instances cannot be launched by this launcher yet")]
    UnsupportedLoader(LoaderKind),
    #[error(transparent)]
    Version(#[from] VersionSourceError),
    #[error("the instance is not installed: {0} is missing")]
    NotInstalled(String),
    #[error(transparent)]
    Java(#[from] JavaRuntimeError),
    #[error(transparent)]
    Natives(#[from] ExtractError),
    #[error(transparent)]
    FileSystem(#[from] FileSystemError),
    #[error(transparent)]
    Process(#[from] GameProcessError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub instance: InstanceId,
    /// Overrides the instance's default account and the active account.
    pub account: Option<AccountId>,
    /// `host[:port]` to join on start (Quick Play, or `--server` on old versions).
    pub join_server: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchOutcome {
    pub exit: GameExit,
    pub played_secs: u64,
}

/// Everything the launcher talks to.
pub struct LaunchDeps<'a, I, A, V, J, N, P, C, F, Q> {
    pub instances: &'a I,
    pub accounts: &'a A,
    pub versions: &'a V,
    pub java: &'a J,
    pub natives: &'a N,
    pub runner: &'a P,
    pub clock: &'a C,
    pub fs: &'a F,
    pub paths: &'a LauncherPaths,
    pub platform: &'a Platform,
    /// Game tokens of premium accounts, refreshed silently when needed.
    pub sessions: &'a Q,
    pub launcher_name: &'a str,
    pub launcher_version: &'a str,
}

struct NoProgress;

impl ProgressReporter for NoProgress {
    fn report(&self, _update: ProgressUpdate) {}
}

/// Offline profiles need a non-empty token; the game never validates it.
const OFFLINE_ACCESS_TOKEN: &str = "0";

#[allow(clippy::too_many_lines)]
pub async fn launch_instance<I, A, V, J, N, P, C, F, Q, S>(
    request: &LaunchRequest,
    deps: &LaunchDeps<'_, I, A, V, J, N, P, C, F, Q>,
    logs: S,
    kill: CancellationToken,
) -> Result<LaunchOutcome, LaunchError>
where
    I: InstanceRepository,
    A: AccountRepository,
    V: VersionSource,
    J: JavaRuntimeProvider,
    N: NativesExtractor,
    P: GameProcessRunner,
    C: Clock,
    F: FileSystem,
    Q: PremiumSessions,
    S: LogSink,
{
    let instance = deps.instances.get(&request.instance).await?;

    let account_id = match request.account.or(instance.default_account) {
        Some(id) => id,
        None => deps
            .accounts
            .active()
            .await?
            .ok_or(LaunchError::NoAccount)?,
    };
    let account = deps
        .accounts
        .list()
        .await?
        .into_iter()
        .find(|account| account.id == account_id)
        .ok_or(LaunchError::NoAccount)?;
    // Before any file work: an expired session should ask for a new sign-in
    // right away, not after preparing the game.
    let (access_token, user_type, xuid) = match &account.kind {
        AccountKind::Microsoft(microsoft) => {
            let token = deps.sessions.session_token(&account).await?;
            (token.expose().to_owned(), "msa", microsoft.xuid.clone())
        }
        AccountKind::Offline => (OFFLINE_ACCESS_TOKEN.to_owned(), "legacy", "0".to_owned()),
    };

    // The installed loader profile (or the game version for vanilla), known
    // without the network so installed instances launch offline.
    let version_id = profile_id(&instance.loader, &instance.game_version)
        .ok_or(LaunchError::UnsupportedLoader(instance.loader.kind()))?;
    let spec = deps.versions.version(version_id.as_str()).await?;
    let plan = plan_libraries(&spec, deps.platform, &Features::new());
    let libraries_dir = deps.paths.libraries_dir();
    let assets_dir = deps.paths.assets_dir();

    let mut classpath: Vec<PathBuf> = plan
        .classpath
        .iter()
        .map(|artifact| artifact.path.under(&libraries_dir))
        .collect();
    if let Some(jar) = &spec.client_jar {
        classpath.push(jar.path.under(&deps.paths.versions_dir()));
    }
    let natives_jars: Vec<(PathBuf, _)> = plan
        .natives
        .iter()
        .map(|native| (native.artifact.path.under(&libraries_dir), native.clone()))
        .collect();
    let logging_config = spec
        .logging
        .as_ref()
        .map(|logging| logging.file.path.under(&assets_dir));
    let asset_index_file = spec
        .asset_index
        .as_ref()
        .map(|index| index.artifact.path.under(&assets_dir));

    // Hashes are verified at install time; refuse to start with anything missing.
    let required = classpath
        .iter()
        .chain(natives_jars.iter().map(|(path, _)| path))
        .chain(logging_config.iter())
        .chain(asset_index_file.iter());
    for path in required {
        if !deps.fs.exists(path).await {
            return Err(LaunchError::NotInstalled(path.display().to_string()));
        }
    }

    let java = match &instance.settings.java {
        JavaSelection::Managed => {
            let requirement = spec.java.clone().unwrap_or_else(JavaRequirement::legacy);
            deps.java
                .ensure(
                    &requirement,
                    deps.platform,
                    &NoProgress,
                    &CancellationToken::new(),
                )
                .await?
                .executable
        }
        JavaSelection::Custom(path) => path.clone(),
    };

    let game_dir = deps.paths.game_dir(&instance.id);
    let natives_dir = deps.paths.natives_dir(&instance.id);
    deps.fs.create_dir_all(&game_dir).await?;
    deps.fs.remove_dir_all(&natives_dir).await?;
    deps.fs.create_dir_all(&natives_dir).await?;
    deps.natives.extract(&natives_jars, &natives_dir).await?;

    let context = LaunchContext {
        platform: deps.platform.clone(),
        identity: LaunchIdentity {
            player_name: account.username.to_string(),
            uuid: account.profile_uuid,
            access_token,
            user_type,
            xuid,
        },
        game_dir: game_dir.clone(),
        assets_root: assets_dir,
        natives_dir,
        libraries_dir,
        classpath,
        launcher_name: deps.launcher_name.to_owned(),
        launcher_version: deps.launcher_version.to_owned(),
        memory: instance.settings.memory,
        extra_jvm_args: instance.settings.jvm_args.clone(),
        resolution: instance.settings.window,
        quick_play_multiplayer: request.join_server.clone(),
        logging_config,
    };
    let command = build_launch_command(&spec, &context);
    let env = instance
        .settings
        .env
        .iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

    let started = deps.clock.now();
    let exit = deps
        .runner
        .run(
            ProcessSpec {
                java,
                command,
                working_dir: game_dir,
                env,
            },
            logs,
            kill,
        )
        .await?;
    let played_secs = deps
        .clock
        .now()
        .duration_since(started)
        .unwrap_or(Duration::ZERO)
        .as_secs();

    // Reload so concurrent edits made while playing are not overwritten.
    let mut latest = deps.instances.get(&request.instance).await?;
    latest.record_playtime(played_secs);
    deps.instances.save(&latest).await?;

    Ok(LaunchOutcome { exit, played_secs })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::*;
    use crate::domain::account::{Account, Username};
    use crate::domain::instance::{GameVersion, Instance, InstanceName, InstanceOrigin};
    use crate::domain::java::{JavaRuntime, JavaSource, JavaVersion};
    use crate::domain::launch::{Arch, LogStream, OsName, VersionManifest};
    use crate::domain::loader::LoaderSpec;
    use crate::testing::{
        FakeFileSystem, FakeGameProcess, FakeJavaRuntimes, FakeSessions, FakeVersionSource,
        FixedClock, InMemoryAccountRepository, InMemoryInstanceRepository, RecordingLogSink,
        RecordingNativesExtractor, sample_asset_index, sample_profile, sample_version_spec,
    };

    struct World {
        instances: InMemoryInstanceRepository,
        accounts: InMemoryAccountRepository,
        versions: FakeVersionSource,
        java: FakeJavaRuntimes,
        natives: RecordingNativesExtractor,
        runner: FakeGameProcess,
        clock: FixedClock,
        fs: FakeFileSystem,
        paths: LauncherPaths,
        platform: Platform,
        account: Account,
        sessions: FakeSessions,
    }

    type Deps<'a> = LaunchDeps<
        'a,
        InMemoryInstanceRepository,
        InMemoryAccountRepository,
        FakeVersionSource,
        FakeJavaRuntimes,
        RecordingNativesExtractor,
        FakeGameProcess,
        FixedClock,
        FakeFileSystem,
        FakeSessions,
    >;

    impl World {
        async fn installed() -> Self {
            let instances = InMemoryInstanceRepository::default();
            instances
                .save(&Instance::new(
                    InstanceId::parse("cinepol").unwrap(),
                    InstanceName::parse("CINEPOL").unwrap(),
                    GameVersion::parse("26.2").unwrap(),
                    LoaderSpec::vanilla(),
                    InstanceOrigin::CinepolServer,
                ))
                .await
                .unwrap();
            let account = Account::offline(
                AccountId::new_random(),
                Username::parse("polispol1").unwrap(),
            );
            let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
            accounts.set_active(Some(account.id)).await.unwrap();
            let paths = LauncherPaths::new("/data");
            let fs = FakeFileSystem::with([
                paths
                    .libraries_dir()
                    .join("org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1.jar"),
                paths.versions_dir().join("26.2/26.2.jar"),
                paths.assets_dir().join("log_configs/client-1.21.2.xml"),
                paths.assets_dir().join("indexes/32.json"),
            ]);
            Self {
                instances,
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
                natives: RecordingNativesExtractor::default(),
                runner: FakeGameProcess::exiting_with_output(
                    0,
                    vec![(
                        LogStream::Stdout,
                        "[Render thread/INFO]: Setting user: polispol1".into(),
                    )],
                ),
                clock: FixedClock::at_unix(1_000),
                fs,
                paths,
                platform: Platform::new(OsName::Linux, Arch::X86_64),
                account,
                sessions: FakeSessions::default(),
            }
        }

        fn deps(&self) -> Deps<'_> {
            LaunchDeps {
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
                sessions: &self.sessions,
                launcher_name: "cinepol-launcher",
                launcher_version: "0.1.0",
            }
        }
    }

    fn request() -> LaunchRequest {
        LaunchRequest {
            instance: InstanceId::parse("cinepol").unwrap(),
            account: None,
            join_server: None,
        }
    }

    #[tokio::test]
    async fn launches_with_the_active_offline_account_and_streams_logs() {
        let world = World::installed().await;
        let logs = RecordingLogSink::default();
        let outcome = launch_instance(
            &request(),
            &world.deps(),
            logs.clone(),
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert!(outcome.exit.success());
        let spec = &world.runner.specs()[0];
        assert_eq!(spec.java, PathBuf::from("/data/runtimes/java"));
        assert_eq!(
            spec.working_dir,
            Path::new("/data/instances/cinepol/minecraft")
        );
        let args = spec.command.args();
        assert!(args.contains(&"polispol1".to_owned()));
        assert!(args.iter().any(|arg| arg == "/data/libraries/org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1.jar:/data/versions/26.2/26.2.jar"));
        assert!(args.contains(
            &"-Dlog4j.configurationFile=/data/assets/log_configs/client-1.21.2.xml".to_owned()
        ));
        assert_eq!(logs.lines().len(), 1);
        assert_eq!(
            world.natives.calls()[0].1,
            PathBuf::from("/data/instances/cinepol/natives")
        );
        assert!(
            world
                .fs
                .removed()
                .contains(&PathBuf::from("/data/instances/cinepol/natives")),
            "stale natives are cleared"
        );
        assert!(
            !args.contains(&"--quickPlayMultiplayer".to_owned()),
            "no server unless asked"
        );
    }

    #[tokio::test]
    async fn refuses_to_launch_when_a_file_is_missing() {
        let world = World::installed().await;
        world
            .fs
            .remove_dir_all(&world.paths.versions_dir())
            .await
            .unwrap();
        let error = launch_instance(
            &request(),
            &world.deps(),
            RecordingLogSink::default(),
            CancellationToken::new(),
        )
        .await;
        assert!(
            matches!(error, Err(LaunchError::NotInstalled(path)) if path.ends_with("26.2.jar"))
        );
        assert!(world.runner.specs().is_empty());
    }

    #[tokio::test]
    async fn requires_an_account_and_a_valid_premium_session() {
        let world = World::installed().await;
        world.accounts.set_active(None).await.unwrap();
        assert_eq!(
            launch_instance(
                &request(),
                &world.deps(),
                RecordingLogSink::default(),
                CancellationToken::new()
            )
            .await,
            Err(LaunchError::NoAccount)
        );

        let mut premium = world.account.clone();
        premium.id = AccountId::new_random();
        premium.kind = AccountKind::Microsoft(crate::domain::account::MicrosoftAccount {
            xuid: "2535".into(),
            skin: None,
        });
        world.accounts.save(&premium).await.unwrap();
        let with_premium = LaunchRequest {
            account: Some(premium.id),
            ..request()
        };
        assert_eq!(
            launch_instance(
                &with_premium,
                &world.deps(),
                RecordingLogSink::default(),
                CancellationToken::new()
            )
            .await,
            Err(LaunchError::Session(SessionError::Expired)),
            "an expired session asks for a new sign-in instead of launching"
        );
        assert!(world.runner.specs().is_empty());
        assert_eq!(world.sessions.asked(), [premium.id]);
    }

    #[tokio::test]
    async fn a_premium_account_launches_with_its_session_token_and_xuid() {
        let mut world = World::installed().await;
        world.sessions = FakeSessions::with_token("minecraft-token");
        let premium = Account::microsoft(AccountId::new_random(), &sample_profile(), "2535428");
        world.accounts.save(&premium).await.unwrap();
        world.accounts.set_active(Some(premium.id)).await.unwrap();

        launch_instance(
            &request(),
            &world.deps(),
            RecordingLogSink::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap();

        let args = world.runner.specs()[0].command.args();
        let after = |flag: &str| {
            let at = args.iter().position(|arg| arg == flag).unwrap();
            args[at + 1].clone()
        };
        assert_eq!(after("--accessToken"), "minecraft-token");
        assert_eq!(after("--username"), "Polispol1");
        assert_eq!(after("--uuid"), premium.profile_uuid.simple().to_string());
        assert_eq!(after("--xuid"), "2535428");
        assert!(
            !world.runner.specs()[0]
                .command
                .redacted_args()
                .iter()
                .any(|arg| arg.contains("minecraft-token"))
        );
    }

    #[tokio::test]
    async fn records_playtime_after_the_game_exits() {
        struct AdvancingRunner<'a>(&'a FixedClock);
        impl GameProcessRunner for AdvancingRunner<'_> {
            async fn run<S: LogSink>(
                &self,
                _spec: ProcessSpec,
                _logs: S,
                _kill: CancellationToken,
            ) -> Result<GameExit, GameProcessError> {
                self.0.advance(Duration::from_secs(95));
                Ok(GameExit {
                    code: Some(0),
                    killed_by_launcher: false,
                })
            }
        }
        let world = World::installed().await;
        let runner = AdvancingRunner(&world.clock);
        let deps = LaunchDeps {
            instances: &world.instances,
            accounts: &world.accounts,
            versions: &world.versions,
            java: &world.java,
            natives: &world.natives,
            runner: &runner,
            clock: &world.clock,
            fs: &world.fs,
            paths: &world.paths,
            platform: &world.platform,
            sessions: &world.sessions,
            launcher_name: "cinepol-launcher",
            launcher_version: "0.1.0",
        };
        let outcome = launch_instance(
            &request(),
            &deps,
            RecordingLogSink::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.played_secs, 95);
        assert_eq!(
            world
                .instances
                .get(&request().instance)
                .await
                .unwrap()
                .playtime_secs,
            95
        );
    }

    #[tokio::test]
    async fn passes_the_server_to_join_and_uses_a_custom_java() {
        let world = World::installed().await;
        let mut instance = world.instances.get(&request().instance).await.unwrap();
        instance.settings.java = JavaSelection::Custom(PathBuf::from("/opt/jdk/bin/java"));
        world.instances.save(&instance).await.unwrap();
        let join = LaunchRequest {
            join_server: Some("mc.policarpiers.com".into()),
            ..request()
        };
        launch_instance(
            &join,
            &world.deps(),
            RecordingLogSink::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let spec = &world.runner.specs()[0];
        assert_eq!(spec.java, PathBuf::from("/opt/jdk/bin/java"));
        assert!(world.java.requested().is_empty());
        let args = spec.command.args();
        assert_eq!(
            &args[args.len() - 2..],
            ["--quickPlayMultiplayer", "mc.policarpiers.com"]
        );
    }

    async fn set_loader(world: &World, kind: crate::domain::loader::LoaderKind, version: &str) {
        let id = InstanceId::parse("cinepol").unwrap();
        let mut instance = world.instances.get(&id).await.unwrap();
        instance.loader = LoaderSpec::new(
            kind,
            Some(crate::domain::loader::LoaderVersion::parse(version).unwrap()),
        )
        .unwrap();
        world.instances.save(&instance).await.unwrap();
    }

    #[tokio::test]
    async fn a_loader_instance_launches_its_cached_profile() {
        let mut world = World::installed().await;
        let mut profile = world.versions.versions["26.2"].clone();
        profile.id = "fabric-loader-0.20.0-26.2".into();
        profile.main_class = "net.fabricmc.loader.impl.launch.knot.KnotClient".into();
        world
            .versions
            .versions
            .insert("fabric-loader-0.20.0-26.2".into(), profile);
        set_loader(&world, crate::domain::loader::LoaderKind::Fabric, "0.20.0").await;

        launch_instance(
            &request(),
            &world.deps(),
            RecordingLogSink::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap();

        let args = world.runner.specs()[0].command.args();
        assert!(args.contains(&"net.fabricmc.loader.impl.launch.knot.KnotClient".to_owned()));
        assert!(!args.contains(&"net.minecraft.client.main.Main".to_owned()));
    }

    #[tokio::test]
    async fn a_loader_instance_that_was_never_installed_reports_its_missing_profile() {
        let world = World::installed().await;
        set_loader(&world, crate::domain::loader::LoaderKind::Forge, "47.2.0").await;
        assert_eq!(
            launch_instance(
                &request(),
                &world.deps(),
                RecordingLogSink::default(),
                CancellationToken::new()
            )
            .await,
            Err(LaunchError::Version(VersionSourceError::UnknownVersion(
                "26.2-forge-47.2.0".into()
            )))
        );
        assert!(world.runner.specs().is_empty());
    }
}
