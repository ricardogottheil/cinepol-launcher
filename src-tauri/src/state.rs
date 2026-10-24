//! Composition root: wires infrastructure adapters into use cases.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError, RwLock};

use launcher_core::application::install_instance::InstallDeps;
use launcher_core::application::modpack_files::PackSyncDeps;
use launcher_core::application::refresh_session::{
    MicrosoftSessions, PremiumSessions, SessionError,
};
use launcher_core::domain::account::{Account, OfflinePolicy};
use launcher_core::domain::launch::{LauncherPaths, Platform};
use launcher_core::domain::ports::Secret;
use launcher_core::domain::server::ServerConfig;
use launcher_core::infrastructure::archive::{
    ZipCurseForgeArchive, ZipMrpackArchive, ZipNativesExtractor,
};
use launcher_core::infrastructure::cinepol::{HttpServerConfigSource, parse_server_config};
use launcher_core::infrastructure::curseforge::CurseForgeSource;
use launcher_core::infrastructure::fs::{
    JsonAccountRepository, JsonInstanceRepository, JsonModRegistryStore, JsonPackManifestStore,
    JsonSavedServerRepository, JsonSettingsRepository, StdPackFiles, TokioFileSystem,
};
use launcher_core::infrastructure::http::{DownloadConfig, DownloadManager, ReqwestHttpClient};
use launcher_core::infrastructure::java::{ManagedJavaRuntimes, ProcessJavaProbe};
use launcher_core::infrastructure::keychain::KeyringSecretStore;
use launcher_core::infrastructure::loaders::{
    ForgeLoaderMetadata, LoaderInstallers, MetaLoaderMetadata,
};
use launcher_core::infrastructure::modrinth::ModrinthSource;
use launcher_core::infrastructure::mojang::{MojangPremiumNames, MojangVersionSource};
use launcher_core::infrastructure::msa::MsaClient;
use launcher_core::infrastructure::nbt::NbtServerListStore;
use launcher_core::infrastructure::packwiz::PackwizFiles;
use launcher_core::infrastructure::process::{TokioGameProcessRunner, TokioProcessorRunner};
use launcher_core::infrastructure::server_ping::MinecraftServerPinger;
use launcher_core::infrastructure::time::{SystemClock, TokioDelay};

use crate::error::{AppError, ErrorCode};
use crate::operations::{OperationRegistry, SessionGate};

/// Build-time `requirePremiumForOffline`: CINEPOL accepts non-premium
/// players, so offline profiles are allowed unless the build opts out.
fn require_premium_for_offline() -> bool {
    option_env!("CINEPOL_REQUIRE_PREMIUM_FOR_OFFLINE") == Some("true")
}

/// The Azure app (client) id for Microsoft sign-in: `MSA_CLIENT_ID` at run
/// time (development) or at build time (`build.rs` reads it from `.env`).
/// `None` builds a launcher without Microsoft sign-in.
fn msa_client_id() -> Option<String> {
    std::env::var("MSA_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("MSA_CLIENT_ID").map(str::to_owned))
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}

/// The CurseForge API key: `CURSEFORGE_API_KEY` at run time (development) or
/// at build time (`build.rs` reads it from `.env`; CI passes a secret).
/// `None` builds a launcher without CurseForge.
fn curseforge_api_key() -> Option<String> {
    std::env::var("CURSEFORGE_API_KEY")
        .ok()
        .or_else(|| option_env!("CURSEFORGE_API_KEY").map(str::to_owned))
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
}

/// The keychain service name: the app identifier, so entries are easy to find.
const KEYCHAIN_SERVICE: &str = "com.policarpiers.cinepol-launcher";

/// `${launcher_name}` passed to the game.
pub const LAUNCHER_NAME: &str = "cinepol-launcher";

/// The config shipped with the launcher, used until the remote one is read.
const BUNDLED_SERVER_CONFIG: &[u8] = include_bytes!("../../config/server-config.json");

/// Contact in the User-Agent, as API providers ask (`<user>/<repo>/<version> (<contact>)`).
const USER_AGENT_CONTACT: &str = "https://cinepol.policarpiers.com";

/// The game installer's dependencies, as wired in this app.
pub type AppInstallDeps<'a> = InstallDeps<
    'a,
    JsonInstanceRepository,
    MojangVersionSource<ReqwestHttpClient>,
    DownloadManager,
    AppJavaRuntimes,
    LoaderInstallers<ReqwestHttpClient>,
>;

type AppJavaRuntimes = ManagedJavaRuntimes<ReqwestHttpClient, DownloadManager, ProcessJavaProbe>;

/// Application state shared with every command through `tauri::State`.
pub struct AppState {
    pub operations: OperationRegistry,
    /// Running games by instance id, so the UI can stop them.
    pub games: OperationRegistry,
    /// One Play on CINEPOL session at a time.
    pub cinepol_session: SessionGate,
    pub delay: TokioDelay,
    pub paths: LauncherPaths,
    pub platform: Platform,
    pub offline_policy: OfflinePolicy,
    pub launcher_version: String,
    pub instances: JsonInstanceRepository,
    pub accounts: JsonAccountRepository,
    pub saved_servers: JsonSavedServerRepository,
    pub versions: MojangVersionSource<ReqwestHttpClient>,
    pub downloader: DownloadManager,
    pub java: AppJavaRuntimes,
    pub natives: ZipNativesExtractor,
    pub runner: TokioGameProcessRunner,
    pub clock: SystemClock,
    pub fs: TokioFileSystem,
    pub servers: NbtServerListStore,
    pub pinger: MinecraftServerPinger,
    pub premium_names: MojangPremiumNames<ReqwestHttpClient>,
    /// Microsoft sign-in; `None` when the build has no `MSA_CLIENT_ID`.
    pub msa: Option<MsaClient<ReqwestHttpClient>>,
    /// Account tokens, in the OS keychain only.
    pub secrets: KeyringSecretStore,
    /// Fabric and Quilt profiles, stored next to the vanilla version JSONs.
    pub loaders: LoaderInstallers<ReqwestHttpClient>,
    pub fabric_releases: MetaLoaderMetadata<ReqwestHttpClient>,
    pub quilt_releases: MetaLoaderMetadata<ReqwestHttpClient>,
    pub forge_releases: ForgeLoaderMetadata<ReqwestHttpClient>,
    pub neoforge_releases: ForgeLoaderMetadata<ReqwestHttpClient>,
    pub config_source: HttpServerConfigSource<ReqwestHttpClient>,
    /// The Modrinth catalog (modpack and mod search, versions, file lookups).
    pub modrinth: ModrinthSource<ReqwestHttpClient>,
    pub curseforge: CurseForgeSource<ReqwestHttpClient>,
    pub curseforge_archive: ZipCurseForgeArchive,
    pub mrpack: ZipMrpackArchive,
    /// packwiz packs over HTTP or from a local folder.
    pub packwiz: PackwizFiles<ReqwestHttpClient>,
    pub pack_files: StdPackFiles,
    /// Which files each modpack instance got from its pack.
    pub pack_manifests: JsonPackManifestStore,
    /// The mods the player added one by one to each instance (`mods.json`).
    pub mod_registry: JsonModRegistryStore,
    pub settings: JsonSettingsRepository,
    /// Checks custom Java executables (`java -version`).
    pub java_probe: ProcessJavaProbe,
    /// Java executables the player picked with the native picker this session.
    picked_java: Mutex<HashSet<PathBuf>>,
    pub bundled_config: ServerConfig,
    /// The last config loaded (remote or bundled), used to seed new instances.
    current_config: RwLock<ServerConfig>,
}

impl AppState {
    /// Game tokens for premium accounts, refreshed silently before launch.
    pub fn sessions(&self) -> AppSessions<'_> {
        AppSessions(self)
    }

    pub fn current_config(&self) -> ServerConfig {
        self.current_config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// What installing an instance (game, loader, Java) needs.
    pub fn install_deps(&self) -> AppInstallDeps<'_> {
        InstallDeps {
            instances: &self.instances,
            versions: &self.versions,
            downloader: &self.downloader,
            java: &self.java,
            paths: &self.paths,
            platform: &self.platform,
            loaders: &self.loaders,
        }
    }

    /// What syncing a pack into an instance's game folder needs.
    pub fn pack_sync(
        &self,
    ) -> PackSyncDeps<
        '_,
        ZipMrpackArchive,
        DownloadManager,
        TokioFileSystem,
        StdPackFiles,
        JsonPackManifestStore,
    > {
        PackSyncDeps {
            archive: &self.mrpack,
            downloader: &self.downloader,
            fs: &self.fs,
            files: &self.pack_files,
            manifests: &self.pack_manifests,
            paths: &self.paths,
        }
    }

    pub fn remember_picked_java(&self, path: PathBuf) {
        self.picked_java
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path);
    }

    pub fn was_picked_java(&self, path: &Path) -> bool {
        self.picked_java
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(path)
    }

    pub fn remember_config(&self, config: ServerConfig) {
        *self
            .current_config
            .write()
            .unwrap_or_else(PoisonError::into_inner) = config;
    }

    /// `data_dir` is the launcher's data root (`<OS data dir>/CinepolLauncher`).
    pub fn new(data_dir: PathBuf, launcher_version: &str) -> Result<Self, AppError> {
        let paths = LauncherPaths::new(data_dir);
        let user_agent =
            format!("ricardogottheil/cinepol-launcher/{launcher_version} ({USER_AGENT_CONTACT})");
        let http = ReqwestHttpClient::new(user_agent)
            .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))?;
        let bundled_config = parse_server_config(BUNDLED_SERVER_CONFIG).map_err(|e| {
            AppError::new(ErrorCode::Internal, format!("bundled server config: {e}"))
        })?;
        let downloader = DownloadManager::new(http.inner().clone(), DownloadConfig::default());
        Ok(Self {
            operations: OperationRegistry::default(),
            games: OperationRegistry::default(),
            cinepol_session: SessionGate::default(),
            delay: TokioDelay,
            platform: Platform::current(),
            offline_policy: OfflinePolicy {
                require_premium_for_offline: require_premium_for_offline(),
            },
            launcher_version: launcher_version.to_owned(),
            instances: JsonInstanceRepository::new(paths.clone()),
            accounts: JsonAccountRepository::new(paths.accounts_file()),
            saved_servers: JsonSavedServerRepository::new(paths.saved_servers_file()),
            versions: MojangVersionSource::new(http.clone(), paths.clone()),
            config_source: HttpServerConfigSource::new(http.clone()),
            modrinth: ModrinthSource::new(http.clone()),
            curseforge: CurseForgeSource::new(http.clone(), curseforge_api_key().as_deref()),
            curseforge_archive: ZipCurseForgeArchive::default(),
            mrpack: ZipMrpackArchive::new(),
            packwiz: PackwizFiles::new(http.clone()),
            pack_files: StdPackFiles,
            pack_manifests: JsonPackManifestStore::new(paths.clone()),
            mod_registry: JsonModRegistryStore::new(paths.clone()),
            settings: JsonSettingsRepository::new(paths.settings_file()),
            java_probe: ProcessJavaProbe::new(),
            picked_java: Mutex::default(),
            servers: NbtServerListStore::new(paths.clone()),
            pinger: MinecraftServerPinger::new(),
            premium_names: MojangPremiumNames::new(http.clone()),
            msa: msa_client_id().map(|client_id| MsaClient::new(http.clone(), client_id)),
            secrets: KeyringSecretStore::new(KEYCHAIN_SERVICE),
            loaders: LoaderInstallers::new(http.clone(), paths.clone(), TokioProcessorRunner),
            fabric_releases: MetaLoaderMetadata::fabric(http.clone()),
            quilt_releases: MetaLoaderMetadata::quilt(http.clone()),
            forge_releases: ForgeLoaderMetadata::forge(http.clone()),
            neoforge_releases: ForgeLoaderMetadata::neoforge(http.clone()),
            current_config: RwLock::new(bundled_config.clone()),
            bundled_config,
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

/// Premium sessions of the running app: Microsoft sign-in when configured.
pub struct AppSessions<'a>(&'a AppState);

impl PremiumSessions for AppSessions<'_> {
    async fn session_token(&self, account: &Account) -> Result<Secret, SessionError> {
        let Some(auth) = &self.0.msa else {
            // Without sign-in no session can be renewed.
            return Err(SessionError::Expired);
        };
        MicrosoftSessions {
            auth,
            secrets: &self.0.secrets,
            clock: &self.0.clock,
        }
        .session_token(account)
        .await
    }
}
