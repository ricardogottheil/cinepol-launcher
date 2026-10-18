//! Serializable DTOs exchanged with the frontend, mapped from domain types.
//!
//! Every DTO exported to TypeScript is registered in `crate::bindings`.

use launcher_core::application::account_skin::SkinTexture;
use launcher_core::application::check_premium_name::PremiumNameCheck;
use launcher_core::application::install_instance::InstallSummary;
use launcher_core::application::launch_instance::LaunchOutcome;
use launcher_core::domain::account::{Account, AccountKind};
use launcher_core::domain::account::{DeviceCode, SkinVariant};
use launcher_core::domain::instance::{Instance, InstanceOrigin};
use launcher_core::domain::launch::{LogStream, VersionSummary};
use launcher_core::domain::loader::LoaderRelease;
use launcher_core::domain::ports::{ProgressStage, ProgressUnit, ProgressUpdate};
use serde::Serialize;
use ts_rs::TS;

/// Application metadata shown in the UI (About, footer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProgressStageDto {
    Metadata,
    Libraries,
    Assets,
    Java,
    Loader,
    Natives,
    Launching,
}

impl From<ProgressStage> for ProgressStageDto {
    fn from(stage: ProgressStage) -> Self {
        match stage {
            ProgressStage::Metadata => Self::Metadata,
            ProgressStage::Libraries => Self::Libraries,
            ProgressStage::Assets => Self::Assets,
            ProgressStage::Java => Self::Java,
            ProgressStage::Loader => Self::Loader,
            ProgressStage::Natives => Self::Natives,
            ProgressStage::Launching => Self::Launching,
        }
    }
}

/// One progress step sent through a `Channel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number | null")]
    pub total: Option<u64>,
    pub stage: Option<ProgressStageDto>,
    /// What `done` and `total` count.
    pub unit: ProgressUnitDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProgressUnitDto {
    Steps,
    Bytes,
}

impl From<ProgressUnit> for ProgressUnitDto {
    fn from(unit: ProgressUnit) -> Self {
        match unit {
            ProgressUnit::Steps => Self::Steps,
            ProgressUnit::Bytes => Self::Bytes,
        }
    }
}

impl From<ProgressUpdate> for ProgressEvent {
    fn from(update: ProgressUpdate) -> Self {
        Self {
            done: update.done,
            total: update.total,
            stage: update.stage.map(Into::into),
            unit: update.unit.into(),
        }
    }
}

/// Whether a premium account owns an offline profile's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PremiumNameDto {
    Premium,
    Free,
    Unknown,
}

impl From<PremiumNameCheck> for PremiumNameDto {
    fn from(check: PremiumNameCheck) -> Self {
        match check {
            PremiumNameCheck::Premium => Self::Premium,
            PremiumNameCheck::Free => Self::Free,
            PremiumNameCheck::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AccountKindDto {
    Microsoft,
    Offline,
}

/// What the Accounts screen may offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountPolicyDto {
    /// This build can sign in with Microsoft (it has an `MSA_CLIENT_ID`).
    pub microsoft_available: bool,
    /// Offline profiles need a premium account first.
    pub require_premium_for_offline: bool,
}

/// The code to show while a Microsoft sign-in waits for approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeDto {
    pub user_code: String,
    pub verification_uri: String,
    #[ts(type = "number")]
    pub expires_in_secs: u64,
}

impl From<&DeviceCode> for DeviceCodeDto {
    fn from(code: &DeviceCode) -> Self {
        Self {
            user_code: code.user_code.clone(),
            verification_uri: code.verification_uri.clone(),
            expires_in_secs: code.expires_in_secs,
        }
    }
}

/// A skin texture as a `data:image/png;base64,...` URL (the CSP allows no
/// remote images), with its arm model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SkinDto {
    pub data_url: String,
    pub slim: bool,
}

impl From<&SkinTexture> for SkinDto {
    fn from(skin: &SkinTexture) -> Self {
        use base64::Engine;
        Self {
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&skin.png)
            ),
            slim: skin.variant == SkinVariant::Slim,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    pub id: String,
    pub kind: AccountKindDto,
    pub username: String,
    pub profile_uuid: String,
}

impl From<&Account> for AccountDto {
    fn from(account: &Account) -> Self {
        Self {
            id: account.id.as_uuid().to_string(),
            kind: match account.kind {
                AccountKind::Microsoft(_) => AccountKindDto::Microsoft,
                AccountKind::Offline => AccountKindDto::Offline,
            },
            username: account.username.to_string(),
            profile_uuid: account.profile_uuid.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountsDto {
    pub accounts: Vec<AccountDto>,
    pub active_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameVersionDto {
    pub id: String,
    /// `release`, `snapshot`, `old_beta` or `old_alpha`.
    pub kind: String,
    pub release_time: String,
}

impl From<&VersionSummary> for GameVersionDto {
    fn from(version: &VersionSummary) -> Self {
        Self {
            id: version.id.as_str().to_owned(),
            kind: version.kind.id().to_owned(),
            release_time: version.release_time.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameVersionsDto {
    pub latest_release: String,
    pub versions: Vec<GameVersionDto>,
}

/// One loader release offered for a game version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LoaderReleaseDto {
    pub version: String,
    pub stable: bool,
}

impl From<&LoaderRelease> for LoaderReleaseDto {
    fn from(release: &LoaderRelease) -> Self {
        Self {
            version: release.version.as_str().to_owned(),
            stable: release.stable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InstanceOriginDto {
    Custom,
    CinepolServer,
    Modpack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDto {
    pub id: String,
    pub name: String,
    pub game_version: String,
    /// `vanilla`, `fabric`, `quilt`, `forge` or `neoforge`.
    pub loader: String,
    pub loader_version: Option<String>,
    pub origin: InstanceOriginDto,
    #[ts(type = "number")]
    pub playtime_secs: u64,
}

impl From<&Instance> for InstanceDto {
    fn from(instance: &Instance) -> Self {
        Self {
            id: instance.id.to_string(),
            name: instance.name.to_string(),
            game_version: instance.game_version.as_str().to_owned(),
            loader: instance.loader.kind().id().to_owned(),
            loader_version: instance.loader.version().map(|v| v.as_str().to_owned()),
            origin: match instance.origin {
                InstanceOrigin::Custom => InstanceOriginDto::Custom,
                InstanceOrigin::CinepolServer => InstanceOriginDto::CinepolServer,
                InstanceOrigin::Modpack(_) => InstanceOriginDto::Modpack,
            },
            playtime_secs: instance.playtime_secs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstallSummaryDto {
    pub downloaded_files: u32,
    pub reused_files: u32,
    #[ts(type = "number")]
    pub downloaded_bytes: u64,
    pub java_major: Option<u32>,
}

impl From<&InstallSummary> for InstallSummaryDto {
    fn from(summary: &InstallSummary) -> Self {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            downloaded_files: count(summary.files.downloaded + summary.assets.downloaded),
            reused_files: count(summary.files.reused + summary.assets.reused),
            downloaded_bytes: summary.files.bytes + summary.assets.bytes,
            java_major: summary.java_major,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LogStreamDto {
    Stdout,
    Stderr,
}

/// One line of game output sent through a `Channel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LogLineDto {
    pub stream: LogStreamDto,
    pub line: String,
}

impl LogLineDto {
    pub fn new(stream: LogStream, line: String) -> Self {
        let stream = match stream {
            LogStream::Stdout => LogStreamDto::Stdout,
            LogStream::Stderr => LogStreamDto::Stderr,
        };
        Self { stream, line }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutcomeDto {
    pub instance_id: String,
    pub exit_code: Option<i32>,
    pub killed: bool,
    #[ts(type = "number")]
    pub played_secs: u64,
}

impl LaunchOutcomeDto {
    pub fn new(instance_id: String, outcome: &LaunchOutcome) -> Self {
        Self {
            instance_id,
            exit_code: outcome.exit.code,
            killed: outcome.exit.killed_by_launcher,
            played_secs: outcome.played_secs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommunityLinksDto {
    pub website: String,
    pub twitch: String,
    pub discord: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ConfigOriginDto {
    Remote,
    Bundled,
}

/// The CINEPOL server configuration the UI shows and acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfigDto {
    pub name: String,
    /// `host[:port]` as players type it.
    pub address: String,
    pub min_version: String,
    pub max_version: String,
    pub recommended_version: String,
    pub links: CommunityLinksDto,
    pub origin: ConfigOriginDto,
}

impl ServerConfigDto {
    pub fn new(
        loaded: &launcher_core::application::load_server_config::LoadedServerConfig,
    ) -> Self {
        use launcher_core::application::load_server_config::ConfigOrigin;
        let config = &loaded.config;
        Self {
            name: config.name.clone(),
            address: config.address.to_string(),
            min_version: config.versions.min().as_str().to_owned(),
            max_version: config.versions.max().as_str().to_owned(),
            recommended_version: config.versions.recommended().as_str().to_owned(),
            links: CommunityLinksDto {
                website: config.links.website.to_string(),
                twitch: config.links.twitch.to_string(),
                discord: config.links.discord.to_string(),
            },
            origin: match loaded.origin {
                ConfigOrigin::Remote => ConfigOriginDto::Remote,
                ConfigOrigin::Bundled { .. } => ConfigOriginDto::Bundled,
            },
        }
    }
}

/// The live answer of the server to a status ping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatusDto {
    pub online_players: u32,
    pub max_players: u32,
    pub version_name: String,
    pub motd: String,
    pub latency_ms: Option<u32>,
}

impl From<&launcher_core::domain::server::ServerStatus> for ServerStatusDto {
    fn from(status: &launcher_core::domain::server::ServerStatus) -> Self {
        Self {
            online_players: status.online_players,
            max_players: status.max_players,
            version_name: status.version_name.clone(),
            motd: status.motd.clone(),
            latency_ms: status.latency_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub server_config_url: String,
    pub default_server_config_url: String,
}

impl From<&launcher_core::domain::settings::AppSettings> for SettingsDto {
    fn from(settings: &launcher_core::domain::settings::AppSettings) -> Self {
        Self {
            server_config_url: settings.server_config_url.to_string(),
            default_server_config_url: launcher_core::domain::settings::DEFAULT_SERVER_CONFIG_URL
                .to_owned(),
        }
    }
}
