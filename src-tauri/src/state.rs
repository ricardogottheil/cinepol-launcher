//! Composition root: wires infrastructure adapters into use cases.

use std::path::PathBuf;

use launcher_core::domain::account::OfflinePolicy;
use launcher_core::domain::launch::{LauncherPaths, Platform};
use launcher_core::infrastructure::archive::ZipNativesExtractor;
use launcher_core::infrastructure::fs::{
    JsonAccountRepository, JsonInstanceRepository, TokioFileSystem,
};
use launcher_core::infrastructure::http::{DownloadConfig, DownloadManager, ReqwestHttpClient};
use launcher_core::infrastructure::java::{ManagedJavaRuntimes, ProcessJavaProbe};
use launcher_core::infrastructure::mojang::MojangVersionSource;
use launcher_core::infrastructure::process::TokioGameProcessRunner;
use launcher_core::infrastructure::time::{SystemClock, TokioDelay};

use crate::error::{AppError, ErrorCode};
use crate::operations::OperationRegistry;

/// Build-time `requirePremiumForOffline`: CINEPOL accepts non-premium
/// players, so offline profiles are allowed unless the build opts out.
fn require_premium_for_offline() -> bool {
    option_env!("CINEPOL_REQUIRE_PREMIUM_FOR_OFFLINE") == Some("true")
}

/// Contact in the User-Agent, as API providers ask (`<user>/<repo>/<version> (<contact>)`).
const USER_AGENT_CONTACT: &str = "https://cinepol.policarpiers.com";

/// Application state shared with every command through `tauri::State`.
pub struct AppState {
    pub operations: OperationRegistry,
    /// Running games by instance id, so the UI can stop them.
    pub games: OperationRegistry,
    pub delay: TokioDelay,
    pub paths: LauncherPaths,
    pub platform: Platform,
    pub offline_policy: OfflinePolicy,
    pub launcher_version: String,
    pub instances: JsonInstanceRepository,
    pub accounts: JsonAccountRepository,
    pub versions: MojangVersionSource<ReqwestHttpClient>,
    pub downloader: DownloadManager,
    pub java: ManagedJavaRuntimes<ReqwestHttpClient, DownloadManager, ProcessJavaProbe>,
    pub natives: ZipNativesExtractor,
    pub runner: TokioGameProcessRunner,
    pub clock: SystemClock,
    pub fs: TokioFileSystem,
}

impl AppState {
    /// `data_dir` is the launcher's data root (`<OS data dir>/CinepolLauncher`).
    pub fn new(data_dir: PathBuf, launcher_version: &str) -> Result<Self, AppError> {
        let paths = LauncherPaths::new(data_dir);
        let user_agent =
            format!("ricardogottheil/cinepol-launcher/{launcher_version} ({USER_AGENT_CONTACT})");
        let http = ReqwestHttpClient::new(user_agent)
            .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
        let downloader = DownloadManager::new(http.inner().clone(), DownloadConfig::default());
        Ok(Self {
            operations: OperationRegistry::default(),
            games: OperationRegistry::default(),
            delay: TokioDelay,
            platform: Platform::current(),
            offline_policy: OfflinePolicy {
                require_premium_for_offline: require_premium_for_offline(),
            },
            launcher_version: launcher_version.to_owned(),
            instances: JsonInstanceRepository::new(paths.clone()),
            accounts: JsonAccountRepository::new(paths.accounts_file()),
            versions: MojangVersionSource::new(http.clone(), paths.clone()),
            java: ManagedJavaRuntimes::new(
                http,
                downloader.clone(),
                ProcessJavaProbe::new(),
                paths.runtimes_dir(),
            ),
            downloader,
            natives: ZipNativesExtractor,
            runner: TokioGameProcessRunner,
            clock: SystemClock,
            fs: TokioFileSystem,
            paths,
        })
    }
}
