//! End-to-end flow against the real Mojang (and loader) services:
//! offline profile -> instance -> install (loader profile, client, libraries,
//! assets, Java) -> launch -> wait for the game to log the player -> stop it.
//!
//! Opt-in only (downloads about 1 GB on the first run and opens a game window):
//! `cargo test -p cinepol-launcher-core --features network-tests --test e2e_vanilla -- --nocapture`
//! Set `CINEPOL_E2E_DIR` to reuse a data directory between runs (default:
//! `target/e2e-data` in the workspace), `CINEPOL_E2E_VERSION` to try another
//! version, and `CINEPOL_E2E_LOADER=fabric`, `quilt`, `forge` or `neoforge` to
//! launch through that loader's newest stable (or recommended) release; the
//! game must then log the loader too.
#![cfg(feature = "network-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use launcher_core::application::create_instance::{NewInstance, create_instance};
use launcher_core::application::create_offline_profile::create_offline_profile;
use launcher_core::application::install_instance::{InstallDeps, install_instance};
use launcher_core::application::launch_instance::{LaunchDeps, LaunchRequest, launch_instance};
use launcher_core::application::list_loader_versions::list_loader_versions;
use launcher_core::domain::account::{AccountRepository, OfflinePolicy};
use launcher_core::domain::instance::{InstanceOrigin, InstanceRepository};
use launcher_core::domain::launch::{LauncherPaths, LogSink, LogStream, Platform};
use launcher_core::domain::loader::{LoaderKind, LoaderSpec};
use launcher_core::domain::ports::{ProgressReporter, ProgressUpdate};
use launcher_core::infrastructure::archive::ZipNativesExtractor;
use launcher_core::infrastructure::fs::{
    JsonAccountRepository, JsonInstanceRepository, TokioFileSystem,
};
use launcher_core::infrastructure::http::{DownloadConfig, DownloadManager, ReqwestHttpClient};
use launcher_core::infrastructure::java::{ManagedJavaRuntimes, ProcessJavaProbe};
use launcher_core::infrastructure::loaders::{
    ForgeLoaderMetadata, LoaderInstallers, MetaLoaderMetadata,
};
use launcher_core::infrastructure::mojang::MojangVersionSource;
use launcher_core::infrastructure::process::{TokioGameProcessRunner, TokioProcessorRunner};
use launcher_core::infrastructure::time::SystemClock;
use tokio_util::sync::CancellationToken;

const PLAYER: &str = "polispol1";
/// Generous: the first start after an install compiles shaders and loads resources.
const START_TIMEOUT: Duration = Duration::from_secs(240);

/// The E2E instance is not seeded, so its server list is never touched.
struct NoServerList;

impl launcher_core::domain::server_list::ServerListStore for NoServerList {
    async fn load(
        &self,
        _instance: &launcher_core::domain::instance::InstanceId,
    ) -> Result<
        launcher_core::domain::server_list::ServerList,
        launcher_core::domain::server_list::ServerListStoreError,
    > {
        Ok(Default::default())
    }

    async fn save(
        &self,
        _instance: &launcher_core::domain::instance::InstanceId,
        _list: &launcher_core::domain::server_list::ServerList,
    ) -> Result<(), launcher_core::domain::server_list::ServerListStoreError> {
        Ok(())
    }
}

struct PrintProgress;

impl ProgressReporter for PrintProgress {
    fn report(&self, update: ProgressUpdate) {
        if update.total.is_some_and(|total| update.done == total) {
            println!(
                "progress {:?}: {}/{:?}",
                update.stage, update.done, update.total
            );
        }
    }
}

/// Collects lines and stops the game once the player is logged in.
#[derive(Clone)]
struct WatchingSink {
    lines: Arc<Mutex<Vec<String>>>,
    stop: CancellationToken,
}

impl LogSink for WatchingSink {
    fn line(&self, stream: LogStream, line: String) {
        if stream == LogStream::Stderr
            || line.contains("Setting user")
            || line.contains("Backend library")
        {
            println!("[{stream:?}] {line}");
        }
        if line.contains(&format!("Setting user: {PLAYER}")) {
            // Leave the game a moment to finish booting, then stop it.
            let stop = self.stop.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                stop.cancel();
            });
        }
        self.lines.lock().unwrap().push(line);
    }
}

fn data_dir() -> PathBuf {
    std::env::var_os("CINEPOL_E2E_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/e2e-data"),
        PathBuf::from,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_and_launches_vanilla_offline() {
    let version = std::env::var("CINEPOL_E2E_VERSION").unwrap_or_else(|_| "26.2".to_owned());
    let paths = LauncherPaths::new(data_dir());
    let platform = Platform::current();
    println!("data dir: {}", paths.root().display());

    let http = ReqwestHttpClient::new(
        "ricardogottheil/cinepol-launcher/e2e (https://cinepol.policarpiers.com)",
    )
    .expect("http client");
    let downloader = DownloadManager::new(http.inner().clone(), DownloadConfig::default());
    let versions = MojangVersionSource::new(http.clone(), paths.clone());
    let loaders = LoaderInstallers::new(http.clone(), paths.clone(), TokioProcessorRunner);
    let java = ManagedJavaRuntimes::new(
        http,
        downloader.clone(),
        ProcessJavaProbe::new(),
        paths.runtimes_dir(),
    );
    let instances = JsonInstanceRepository::new(paths.clone());
    let accounts = JsonAccountRepository::new(paths.accounts_file());

    let loader = match std::env::var("CINEPOL_E2E_LOADER").ok().as_deref() {
        None | Some("vanilla") => LoaderSpec::vanilla(),
        Some(raw) => {
            let kind = LoaderKind::from_id(raw).expect("CINEPOL_E2E_LOADER is fabric or quilt");
            let http = ReqwestHttpClient::new(
                "ricardogottheil/cinepol-launcher/e2e (https://cinepol.policarpiers.com)",
            )
            .expect("http client");
            let releases = match kind {
                LoaderKind::Fabric => {
                    list_loader_versions(&version, &MetaLoaderMetadata::fabric(http)).await
                }
                LoaderKind::Quilt => {
                    list_loader_versions(&version, &MetaLoaderMetadata::quilt(http)).await
                }
                LoaderKind::Forge => {
                    list_loader_versions(&version, &ForgeLoaderMetadata::forge(http)).await
                }
                LoaderKind::NeoForge => {
                    list_loader_versions(&version, &ForgeLoaderMetadata::neoforge(http)).await
                }
                LoaderKind::Vanilla => unreachable!("vanilla is handled above"),
            }
            .expect("loader releases");
            let newest_stable = releases
                .into_iter()
                .find(|release| release.stable)
                .expect("a stable loader release");
            LoaderSpec::new(kind, Some(newest_stable.version)).expect("loader spec")
        }
    };
    println!("loader: {loader:?}");

    if accounts.list().await.unwrap().is_empty() {
        create_offline_profile(PLAYER, OfflinePolicy::default(), &accounts)
            .await
            .expect("offline profile");
    }
    let instance = match instances
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|i| i.game_version.as_str() == version && i.loader == loader)
    {
        Some(existing) => existing,
        None => create_instance(
            NewInstance {
                name: &format!("E2E {version} {}", loader.kind().id()),
                game_version: &version,
                loader: loader.clone(),
                origin: InstanceOrigin::Custom,
                seed_server: None,
            },
            &instances,
            &NoServerList,
        )
        .await
        .expect("instance"),
    };

    let summary = install_instance(
        &instance.id,
        &InstallDeps {
            instances: &instances,
            versions: &versions,
            downloader: &downloader,
            java: &java,
            paths: &paths,
            platform: &platform,
            loaders: &loaders,
        },
        &PrintProgress,
        &CancellationToken::new(),
    )
    .await
    .expect("install");
    println!("install: {summary:?}");

    let stop = CancellationToken::new();
    let sink = WatchingSink {
        lines: Arc::default(),
        stop: stop.clone(),
    };
    let deps = LaunchDeps {
        instances: &instances,
        accounts: &accounts,
        versions: &versions,
        java: &java,
        natives: &ZipNativesExtractor,
        runner: &TokioGameProcessRunner,
        clock: &SystemClock,
        fs: &TokioFileSystem,
        paths: &paths,
        platform: &platform,
        sessions: &launcher_core::application::refresh_session::NoPremiumSessions,
        launcher_name: "cinepol-launcher",
        launcher_version: "e2e",
    };
    let request = LaunchRequest {
        instance: instance.id.clone(),
        account: None,
        join_server: None,
    };

    let timeout = {
        let stop = stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(START_TIMEOUT).await;
            stop.cancel();
        })
    };
    let outcome = launch_instance(&request, &deps, sink.clone(), stop)
        .await
        .expect("launch");
    timeout.abort();

    let lines = sink.lines.lock().unwrap().clone();
    println!(
        "exit: {:?}, played {} s, {} log lines",
        outcome.exit,
        outcome.played_secs,
        lines.len()
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains(&format!("Setting user: {PLAYER}"))),
        "the game never logged the player in; last lines: {:#?}",
        lines.iter().rev().take(30).collect::<Vec<_>>()
    );
    if let Some(loader_version) = loader.version() {
        let started = match loader.kind() {
            LoaderKind::Fabric => {
                let expected = format!("with Fabric Loader {}", loader_version.as_str());
                lines.iter().any(|line| line.contains(&expected))
            }
            LoaderKind::Quilt => {
                let expected = format!("with Quilt Loader {}", loader_version.as_str());
                lines.iter().any(|line| line.contains(&expected))
            }
            // Forge and NeoForge log through their own loaders (FML).
            _ => lines
                .iter()
                .any(|line| line.to_ascii_lowercase().contains("forge")),
        };
        assert!(started, "the {:?} loader never started", loader.kind());
    }
    assert!(
        outcome.exit.killed_by_launcher,
        "the game should still be running when the test stops it"
    );
    assert!(instances.get(&instance.id).await.unwrap().playtime_secs >= outcome.played_secs);
}
