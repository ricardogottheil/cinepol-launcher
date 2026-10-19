//! End-to-end packwiz flow against a real packwiz pack and the Mojang services:
//! offline profile -> install a packwiz pack from its pack.toml URL (verified
//! index, metafiles, plain files, manifest) -> install the game and its loader
//! -> launch -> wait for the game to log the player and the loader to load the
//! pack's mods -> stop it.
//!
//! Opt-in only (downloads several hundred MB on the first run and opens a game
//! window):
//! `cargo test -p cinepol-launcher-core --features network-tests --test e2e_packwiz -- --nocapture`
//! Set `CINEPOL_E2E_DIR` to reuse a data directory (default: `target/e2e-data`
//! in the workspace) and `CINEPOL_E2E_PACKWIZ` for another https pack.toml URL
//! (default: Fabulously Optimized 26.2).
#![cfg(feature = "network-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use launcher_core::application::create_offline_profile::create_offline_profile;
use launcher_core::application::install_instance::{InstallDeps, install_instance};
use launcher_core::application::install_packwiz::{
    InstallPackwizDeps, InstallPackwizRequest, install_packwiz,
};
use launcher_core::application::launch_instance::{LaunchDeps, LaunchRequest, launch_instance};
use launcher_core::application::modpack_files::FileSyncDeps;
use launcher_core::domain::account::{AccountRepository, OfflinePolicy};
use launcher_core::domain::launch::{LauncherPaths, LogSink, LogStream, Platform};
use launcher_core::domain::loader::LoaderKind;
use launcher_core::domain::modpack::{PackLocation, PackManifestStore};
use launcher_core::domain::ports::{ProgressReporter, ProgressUpdate};
use launcher_core::infrastructure::archive::ZipNativesExtractor;
use launcher_core::infrastructure::fs::{
    JsonAccountRepository, JsonInstanceRepository, JsonPackManifestStore, StdPackFiles,
    TokioFileSystem,
};
use launcher_core::infrastructure::http::{DownloadConfig, DownloadManager, ReqwestHttpClient};
use launcher_core::infrastructure::java::{ManagedJavaRuntimes, ProcessJavaProbe};
use launcher_core::infrastructure::loaders::LoaderInstallers;
use launcher_core::infrastructure::mojang::MojangVersionSource;
use launcher_core::infrastructure::packwiz::PackwizFiles;
use launcher_core::infrastructure::process::{TokioGameProcessRunner, TokioProcessorRunner};
use launcher_core::infrastructure::time::SystemClock;
use tokio_util::sync::CancellationToken;

const PLAYER: &str = "polispol1";
const FO_PACKWIZ: &str = "https://raw.githubusercontent.com/Fabulously-Optimized/fabulously-optimized/main/Packwiz/26.2/pack.toml";
const USER_AGENT: &str = "ricardogottheil/cinepol-launcher/e2e (https://cinepol.policarpiers.com)";
/// Generous: a modded first start compiles shaders and loads every mod.
const START_TIMEOUT: Duration = Duration::from_secs(360);

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
        if line.contains("Setting user") || line.contains("Loading ") && line.contains(" mods") {
            println!("[{stream:?}] {line}");
        }
        if line.contains(&format!("Setting user: {PLAYER}")) {
            // Leave the game a moment to finish booting, then stop it.
            let stop = self.stop.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(10)).await;
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
async fn installs_and_launches_a_packwiz_pack_offline() {
    let url = std::env::var("CINEPOL_E2E_PACKWIZ").unwrap_or_else(|_| FO_PACKWIZ.to_owned());
    let paths = LauncherPaths::new(data_dir());
    let platform = Platform::current();
    println!("data dir: {}", paths.root().display());

    let http = ReqwestHttpClient::new(USER_AGENT).expect("http client");
    let downloader = DownloadManager::new(http.inner().clone(), DownloadConfig::default());
    let packwiz = PackwizFiles::new(http.clone());
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
    let manifests = JsonPackManifestStore::new(paths.clone());

    println!("packwiz pack {url}");
    if accounts.list().await.unwrap().is_empty() {
        create_offline_profile(PLAYER, OfflinePolicy::default(), &accounts)
            .await
            .expect("offline profile");
    }

    let location = PackLocation::Url(url.clone());
    let installed = install_packwiz(
        InstallPackwizRequest {
            pack: &location,
            name: None,
            seed_server: None,
            claim: None,
        },
        &InstallPackwizDeps {
            instances: &instances,
            servers: &NoServerList,
            source: &packwiz,
            files: FileSyncDeps {
                downloader: &downloader,
                fs: &TokioFileSystem,
                files: &StdPackFiles,
                manifests: &manifests,
                paths: &paths,
            },
        },
        &PrintProgress,
        &CancellationToken::new(),
    )
    .await
    .expect("packwiz install");
    let instance = installed.instance;
    println!(
        "instance {} ({} {:?}), pack files: {:?}, manual downloads: {}",
        instance.id,
        instance.game_version.as_str(),
        instance.loader,
        installed.files,
        installed.pending.len()
    );
    assert!(
        installed.pending.is_empty(),
        "the pack needs manual downloads"
    );
    assert!(installed.files.written > 0, "the pack wrote no files");
    let manifest = manifests.load(&instance.id).await.expect("manifest");
    assert_eq!(manifest.files.len(), installed.files.written);
    let mods_dir = paths.game_dir(&instance.id).join("mods");
    let jars = std::fs::read_dir(&mods_dir)
        .expect("mods folder")
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jar"))
        .count();
    println!("{jars} mod jars in {}", mods_dir.display());
    assert!(jars > 0, "the pack installed no mods");

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
    .expect("game install");
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
        lines.iter().rev().take(40).collect::<Vec<_>>()
    );
    if instance.loader.kind() == LoaderKind::Fabric {
        let loading = lines
            .iter()
            .find(|line| line.contains("Loading ") && line.contains(" mods"))
            .expect("Fabric never listed the mods it loads");
        let count: usize = loading
            .split("Loading ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .expect("a mod count");
        // Fabric counts its own built-ins too, so it loads at least the pack's jars.
        assert!(
            count >= jars,
            "Fabric loaded {count} mods, the pack has {jars} jars"
        );
    }
    assert!(
        outcome.exit.killed_by_launcher,
        "the game should still be running when the test stops it"
    );
}
