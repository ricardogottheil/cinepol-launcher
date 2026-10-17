//! "Play on CINEPOL": prepare the managed instance, install what is missing
//! and launch straight into the server.

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::application::create_instance::CreateInstanceError;
use crate::application::install_instance::{InstallDeps, InstallError, install_instance};
use crate::application::launch_instance::{
    LaunchDeps, LaunchError, LaunchOutcome, LaunchRequest, launch_instance,
};
use crate::application::prepare_cinepol_instance::prepare_cinepol_instance;
use crate::domain::account::AccountRepository;
use crate::domain::instance::{Instance, InstanceRepository};
use crate::domain::java::JavaRuntimeProvider;
use crate::domain::launch::{GameProcessRunner, LogSink, NativesExtractor, VersionSource};
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CinepolSession {
    pub instance: Instance,
    pub outcome: LaunchOutcome,
}

/// Reports a `Launching` step between install and launch so the UI can show
/// the game as starting. `cancel` stops the install; `kill` stops the game.
#[allow(clippy::too_many_arguments)]
pub async fn play_on_cinepol<I, A, V, D, J, N, P, C, F, S, R, L>(
    config: &ServerConfig,
    servers: &S,
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
    R: ProgressReporter,
    L: LogSink,
{
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;
    use crate::application::load_server_config::fixtures::cinepol;
    use crate::domain::account::{Account, AccountId, Username};
    use crate::domain::instance::GameVersion;
    use crate::domain::java::{JavaRuntime, JavaSource, JavaVersion};
    use crate::domain::launch::{Arch, LauncherPaths, OsName, Platform, VersionManifest};
    use crate::testing::{
        FakeDownloader, FakeFileSystem, FakeGameProcess, FakeJavaRuntimes, FakeVersionSource,
        FixedClock, InMemoryAccountRepository, InMemoryInstanceRepository, InMemoryServerListStore,
        RecordingLogSink, RecordingNativesExtractor, RecordingReporter, sample_asset_index,
        sample_version_spec,
    };

    #[tokio::test]
    async fn prepares_installs_and_joins_the_server_with_quick_play() {
        let config = cinepol("26.2");
        let instances = InMemoryInstanceRepository::default();
        let servers = InMemoryServerListStore::default();
        let account = Account::offline(
            AccountId::new_random(),
            Username::parse("polispol1").unwrap(),
        );
        let accounts = InMemoryAccountRepository::with(vec![account.clone()]);
        accounts.set_active(Some(account.id)).await.unwrap();
        let versions = FakeVersionSource {
            manifest: VersionManifest {
                latest_release: GameVersion::parse("26.2").unwrap(),
                latest_snapshot: GameVersion::parse("26.2").unwrap(),
                versions: vec![],
            },
            versions: HashMap::from([("26.2".to_owned(), sample_version_spec("26.2"))]),
            asset_indexes: HashMap::from([("32".to_owned(), sample_asset_index())]),
        };
        let java = FakeJavaRuntimes::providing(JavaRuntime {
            executable: PathBuf::from("/data/runtimes/java"),
            version: JavaVersion {
                major: 25,
                full: "25.0.1".into(),
                is_64_bit: true,
            },
            source: JavaSource::Managed,
        });
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
        let downloader = FakeDownloader::default();
        let natives = RecordingNativesExtractor::default();
        let runner = FakeGameProcess::exiting_with(0);
        let clock = FixedClock::at_unix(1_000);
        let platform = Platform::new(OsName::MacOs, Arch::Arm64);
        let install = InstallDeps {
            instances: &instances,
            versions: &versions,
            downloader: &downloader,
            java: &java,
            paths: &paths,
            platform: &platform,
        };
        let launch = LaunchDeps {
            instances: &instances,
            accounts: &accounts,
            versions: &versions,
            java: &java,
            natives: &natives,
            runner: &runner,
            clock: &clock,
            fs: &fs,
            paths: &paths,
            platform: &platform,
            launcher_name: "cinepol-launcher",
            launcher_version: "0.1.0",
        };
        let reporter = RecordingReporter::default();

        let session = play_on_cinepol(
            &config,
            &servers,
            &install,
            &launch,
            &reporter,
            RecordingLogSink::default(),
            &CancellationToken::new(),
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(session.instance.id.as_str(), "cinepol");
        assert!(session.outcome.exit.success());
        assert!(
            !downloader.requests().is_empty(),
            "installed before launching"
        );
        let args = runner.specs()[0].command.args();
        assert_eq!(
            &args[args.len() - 2..],
            ["--quickPlayMultiplayer", "mc.policarpiers.com"]
        );
        assert_eq!(
            reporter.updates().last().and_then(|u| u.stage),
            Some(ProgressStage::Launching)
        );
        assert_eq!(
            servers
                .load(&session.instance.id)
                .await
                .unwrap()
                .entries()
                .len(),
            1
        );
    }
}
