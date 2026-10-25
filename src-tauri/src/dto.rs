//! Serializable DTOs exchanged with the frontend, mapped from domain types.
//!
//! Every DTO exported to TypeScript is registered in `crate::bindings`.

use launcher_core::application::account_capes::CapeTexture;
use launcher_core::application::account_skin::SkinTexture;
use launcher_core::application::check_mod_updates::{ModUpdate, ModUpdates};
use launcher_core::application::check_premium_name::PremiumNameCheck;
use launcher_core::application::export_mrpack::ExportSummary;
use launcher_core::application::install_instance::InstallSummary;
use launcher_core::application::launch_instance::LaunchOutcome;
use launcher_core::application::modpack_files::SyncSummary;
use launcher_core::domain::account::{Account, AccountKind};
use launcher_core::domain::account::{DeviceCode, SkinVariant};
use launcher_core::domain::instance::{Instance, InstanceOrigin, JavaSelection};
use launcher_core::domain::launch::log4j::{LogEntry, LogLevel};
use launcher_core::domain::launch::{LogStream, VersionSummary};
use launcher_core::domain::loader::LoaderRelease;
use launcher_core::domain::modpack::sync::ManualFile;
use launcher_core::domain::modpack::{
    ModpackPage, ModpackSourceKind, ModpackSummary, ModpackVersion,
};
use launcher_core::domain::mods::{
    InstalledMod, ModEntry, ModInstallPlan, ModOrigin, ModPage, ModSummary, mod_catalog,
};
use launcher_core::domain::ports::{ProgressStage, ProgressUnit, ProgressUpdate};
use launcher_core::domain::server_list::ServerList;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Application metadata shown in the UI (About, footer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// What a launcher update check found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AppUpdateStatusDto {
    /// This build has no update key (development builds).
    NotConfigured,
    UpToDate,
    #[serde(rename_all = "camelCase")]
    Available {
        version: String,
        current_version: String,
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
    },
}

impl From<launcher_core::domain::update::UpdateStatus> for AppUpdateStatusDto {
    fn from(status: launcher_core::domain::update::UpdateStatus) -> Self {
        use launcher_core::domain::update::UpdateStatus;
        match status {
            UpdateStatus::NotConfigured => Self::NotConfigured,
            UpdateStatus::UpToDate => Self::UpToDate,
            UpdateStatus::Available(update) => Self::Available {
                version: update.version,
                current_version: update.current_version,
                notes: update.notes,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProgressStageDto {
    Metadata,
    Libraries,
    Assets,
    Java,
    Loader,
    Modpack,
    Mods,
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
            ProgressStage::Modpack => Self::Modpack,
            ProgressStage::Mods => Self::Mods,
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

fn png_data_url(png: &[u8]) -> String {
    use base64::Engine;
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

impl From<&SkinTexture> for SkinDto {
    fn from(skin: &SkinTexture) -> Self {
        Self {
            data_url: png_data_url(&skin.png),
            slim: skin.variant == SkinVariant::Slim,
        }
    }
}

/// A cape the profile owns; `dataUrl` is `null` when its texture did not load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CapeDto {
    pub id: String,
    pub alias: String,
    pub active: bool,
    pub data_url: Option<String>,
}

impl From<&CapeTexture> for CapeDto {
    fn from(cape: &CapeTexture) -> Self {
        Self {
            id: cape.id.as_str().to_owned(),
            alias: cape.alias.clone(),
            active: cape.active,
            data_url: cape.png.as_deref().map(png_data_url),
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

/// The id the UI knows a modpack source by.
pub fn source_id(kind: ModpackSourceKind) -> &'static str {
    match kind {
        ModpackSourceKind::Modrinth => "modrinth",
        ModpackSourceKind::CurseForge => "curseforge",
        ModpackSourceKind::Packwiz => "packwiz",
    }
}

/// A modpack in a catalog listing or details view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModpackSummaryDto {
    /// `modrinth` or `curseforge`.
    pub source: String,
    pub project: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    #[ts(type = "number | null")]
    pub downloads: Option<u64>,
    /// Loader ids (`fabric`, `quilt`, `forge`, `neoforge`).
    pub loaders: Vec<String>,
}

impl From<&ModpackSummary> for ModpackSummaryDto {
    fn from(summary: &ModpackSummary) -> Self {
        Self {
            source: source_id(summary.source).to_owned(),
            project: summary.project.clone(),
            title: summary.title.clone(),
            description: summary.description.clone(),
            icon_url: summary.icon_url.clone(),
            downloads: summary.downloads,
            loaders: summary
                .loaders
                .iter()
                .map(|loader| loader.id().to_owned())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModpackPageDto {
    pub items: Vec<ModpackSummaryDto>,
    #[ts(type = "number | null")]
    pub total: Option<u64>,
}

impl From<&ModpackPage> for ModpackPageDto {
    fn from(page: &ModpackPage) -> Self {
        Self {
            items: page.items.iter().map(ModpackSummaryDto::from).collect(),
            total: page.total,
        }
    }
}

/// A mod in a catalog search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModSummaryDto {
    pub project: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    #[ts(type = "number | null")]
    pub downloads: Option<u64>,
}

impl From<&ModSummary> for ModSummaryDto {
    fn from(summary: &ModSummary) -> Self {
        Self {
            project: summary.project.clone(),
            title: summary.title.clone(),
            description: summary.description.clone(),
            icon_url: summary.icon_url.clone(),
            downloads: summary.downloads,
        }
    }
}

/// One page of mods that run on the instance searched for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModPageDto {
    pub items: Vec<ModSummaryDto>,
    #[ts(type = "number | null")]
    pub total: Option<u64>,
}

impl From<&ModPage> for ModPageDto {
    fn from(page: &ModPage) -> Self {
        Self {
            items: page.items.iter().map(ModSummaryDto::from).collect(),
            total: page.total,
        }
    }
}

/// A mod an install would write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlannedModDto {
    pub project: String,
    pub title: String,
    pub version_number: String,
    /// Whether it is the mod the player picked, rather than a dependency.
    pub requested: bool,
    /// Titles of the planned mods that need it.
    pub required_by: Vec<String>,
}

/// What installing a mod would write: the picked mod first, then the
/// required dependencies the instance does not have yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModInstallPreviewDto {
    pub mods: Vec<PlannedModDto>,
}

impl From<&ModInstallPlan> for ModInstallPreviewDto {
    fn from(plan: &ModInstallPlan) -> Self {
        let title_of = |project: &String| {
            plan.mods
                .iter()
                .find(|planned| &planned.version.project == project)
                .map_or_else(|| project.clone(), |planned| planned.title.clone())
        };
        Self {
            mods: plan
                .mods
                .iter()
                .map(|planned| PlannedModDto {
                    project: planned.version.project.clone(),
                    title: planned.title.clone(),
                    version_number: planned.version.version_number.clone(),
                    requested: planned.requested,
                    required_by: planned.required_by.iter().map(title_of).collect(),
                })
                .collect(),
        }
    }
}

/// A mod an install or update wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModDto {
    pub project: String,
    pub title: String,
    pub version_number: String,
    pub filename: String,
}

impl From<&InstalledMod> for InstalledModDto {
    fn from(installed: &InstalledMod) -> Self {
        Self {
            project: installed.project.clone(),
            title: installed.title.clone(),
            version_number: installed.version_number.clone(),
            filename: installed.filename.clone(),
        }
    }
}

/// Where a jar in `mods/` came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ModOriginDto {
    /// Installed with the launcher's mod browser.
    Launcher,
    /// Added by hand or by a pack, and recognised by Modrinth.
    Identified,
    /// Not known to Modrinth.
    Unknown,
}

impl From<ModOrigin> for ModOriginDto {
    fn from(origin: ModOrigin) -> Self {
        match origin {
            ModOrigin::Launcher => Self::Launcher,
            ModOrigin::Identified => Self::Identified,
            ModOrigin::Unknown => Self::Unknown,
        }
    }
}

/// A jar in an instance's `mods/` folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModEntryDto {
    /// The jar's name without `.disabled`; commands take this name.
    pub filename: String,
    pub enabled: bool,
    /// The mod's title, or the file name when it is unknown.
    pub title: String,
    pub origin: ModOriginDto,
    pub project: Option<String>,
    /// The catalog the launcher installed it from (`modrinth` or
    /// `curseforge`); absent for loose jars.
    pub source: Option<String>,
    pub version_number: Option<String>,
    /// Whether the player added it, rather than another mod needing it.
    pub requested: bool,
    /// Titles of the installed mods that need it.
    pub required_by: Vec<String>,
    /// Titles of the dependencies nothing else would need once it is removed.
    pub orphans: Vec<String>,
}

impl ModEntryDto {
    /// Maps every entry, naming projects by the titles the list knows.
    pub fn list(entries: &[ModEntry]) -> Vec<Self> {
        let title_of = |project: &String| {
            entries
                .iter()
                .filter_map(|entry| entry.record.as_ref())
                .find(|record| &record.project == project)
                .map_or_else(|| project.clone(), |record| record.title.clone())
        };
        entries
            .iter()
            .map(|entry| {
                let record = entry.record.as_ref();
                Self {
                    filename: entry.filename.clone(),
                    enabled: entry.enabled,
                    title: entry.title().to_owned(),
                    origin: entry.origin.into(),
                    project: record.map(|record| record.project.clone()),
                    source: record.map(|record| source_id(mod_catalog(&record.project)).to_owned()),
                    version_number: record.map(|record| record.version_number.clone()),
                    requested: record.is_none_or(|record| record.requested),
                    required_by: record
                        .map(|record| record.required_by.iter().map(title_of).collect())
                        .unwrap_or_default(),
                    orphans: entry.orphans.iter().map(title_of).collect(),
                }
            })
            .collect()
    }
}

/// A newer compatible version of an installed mod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdateDto {
    pub project: String,
    pub title: String,
    pub filename: String,
    /// The installed version label.
    pub current: String,
    /// The version label the update installs.
    pub version_number: String,
}

impl From<&ModUpdate> for ModUpdateDto {
    fn from(update: &ModUpdate) -> Self {
        Self {
            project: update.project.clone(),
            title: update.title.clone(),
            filename: update.filename.clone(),
            current: update.current.clone(),
            version_number: update.version.version_number.clone(),
        }
    }
}

/// What an update check found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdatesDto {
    pub updates: Vec<ModUpdateDto>,
    /// Titles of the mods Modrinth could not answer for; they may have an
    /// update too, so the UI says so instead of showing them as current.
    pub unchecked: Vec<String>,
}

impl From<&ModUpdates> for ModUpdatesDto {
    fn from(checked: &ModUpdates) -> Self {
        Self {
            updates: checked.updates.iter().map(ModUpdateDto::from).collect(),
            unchecked: checked.unchecked.clone(),
        }
    }
}

/// One release of a modpack; `installable` is false when it has no `.mrpack`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModpackVersionDto {
    pub id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    /// Loader ids (`fabric`, `quilt`, `forge`, `neoforge`).
    pub loaders: Vec<String>,
    pub published: Option<String>,
    pub installable: bool,
    #[ts(type = "number | null")]
    pub size: Option<u64>,
}

impl From<&ModpackVersion> for ModpackVersionDto {
    fn from(version: &ModpackVersion) -> Self {
        Self {
            id: version.id.clone(),
            name: version.name.clone(),
            version_number: version.version_number.clone(),
            game_versions: version
                .game_versions
                .iter()
                .map(|v| v.as_str().to_owned())
                .collect(),
            loaders: version.loaders.iter().map(|l| l.id().to_owned()).collect(),
            published: version.published.clone(),
            installable: version.pack_file.is_some(),
            size: version.pack_file.as_ref().map(|file| file.size),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModpackDetailsDto {
    pub summary: ModpackSummaryDto,
    pub versions: Vec<ModpackVersionDto>,
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
    /// For modpack instances: `modrinth`, `curseforge` or `packwiz`.
    pub modpack_source: Option<String>,
    /// packwiz instances sync their pack before every launch.
    pub update_pack_on_launch: bool,
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
            modpack_source: match &instance.origin {
                InstanceOrigin::Modpack(pack) => Some(source_id(pack.source).to_owned()),
                _ => None,
            },
            update_pack_on_launch: instance.settings.update_pack_on_launch,
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
    /// Set for log4j events; plain lines have none.
    pub level: Option<LogLevelDto>,
    pub thread: Option<String>,
    pub logger: Option<String>,
}

/// A log4j event level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LogLevelDto {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

fn stream_dto(stream: LogStream) -> LogStreamDto {
    match stream {
        LogStream::Stdout => LogStreamDto::Stdout,
        LogStream::Stderr => LogStreamDto::Stderr,
    }
}

impl LogLineDto {
    pub fn new(stream: LogStream, line: String) -> Self {
        Self {
            stream: stream_dto(stream),
            line,
            level: None,
            thread: None,
            logger: None,
        }
    }
}

impl From<LogEntry> for LogLineDto {
    fn from(entry: LogEntry) -> Self {
        Self {
            stream: stream_dto(entry.stream),
            line: entry.message,
            level: entry.level.map(|level| match level {
                LogLevel::Trace => LogLevelDto::Trace,
                LogLevel::Debug => LogLevelDto::Debug,
                LogLevel::Info => LogLevelDto::Info,
                LogLevel::Warn => LogLevelDto::Warn,
                LogLevel::Error => LogLevelDto::Error,
                LogLevel::Fatal => LogLevelDto::Fatal,
            }),
            thread: entry.thread,
            logger: entry.logger,
        }
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
    /// File name of the crash report the game wrote, when it crashed.
    pub crash_report: Option<String>,
}

impl LaunchOutcomeDto {
    pub fn new(instance_id: String, outcome: &LaunchOutcome) -> Self {
        Self {
            instance_id,
            exit_code: outcome.exit.code,
            killed: outcome.exit.killed_by_launcher,
            played_secs: outcome.played_secs,
            crash_report: outcome
                .crash_report
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned()),
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

/// What a pack sync did to the game folder; `kept` counts the player's changes left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PackFilesDto {
    pub written: u32,
    pub unchanged: u32,
    pub kept: u32,
    pub removed: u32,
}

impl From<&SyncSummary> for PackFilesDto {
    fn from(summary: &SyncSummary) -> Self {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            written: count(summary.written),
            unchanged: count(summary.unchanged),
            kept: count(summary.kept),
            removed: count(summary.removed),
        }
    }
}

/// A modpack installed into a new instance, with the game and loader ready.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModpackInstallDto {
    pub instance: InstanceDto,
    pub files: PackFilesDto,
    pub install: InstallSummaryDto,
    /// CurseForge files the player has to download by hand.
    pub pending: Vec<ManualFileDto>,
}

/// The modpack catalogs this build can talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModpackCatalogsDto {
    /// `false` in builds without a CurseForge API key.
    pub curseforge: bool,
}

/// `instance` and `files` are absent when the instance was already up to date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModpackUpdateDto {
    pub up_to_date: bool,
    pub instance: Option<InstanceDto>,
    pub files: Option<PackFilesDto>,
    /// CurseForge files the player has to download by hand.
    pub pending: Vec<ManualFileDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModpackExportDto {
    /// Where the `.mrpack` was written.
    pub path: String,
    /// Files listed for download from Modrinth.
    pub indexed: u32,
    /// Files copied into the pack.
    pub overrides: u32,
}

impl ModpackExportDto {
    pub fn new(path: &std::path::Path, summary: &ExportSummary) -> Self {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            path: path.display().to_string(),
            indexed: count(summary.indexed),
            overrides: count(summary.overrides),
        }
    }
}

/// `managed`: the launcher downloads the runtime the version needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum JavaKindDto {
    Managed,
    Custom,
}

/// Which Java an instance runs with. `path` is set only for `custom`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct JavaChoiceDto {
    pub kind: JavaKindDto,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WindowSizeDto {
    pub width: u32,
    pub height: u32,
}

/// Everything the player can edit on an instance; sent back whole on save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSettingsDto {
    pub name: String,
    pub default_account_id: Option<String>,
    pub java: JavaChoiceDto,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub jvm_args: Vec<String>,
    /// `null` keeps the game's own window size.
    pub window: Option<WindowSizeDto>,
    pub env: std::collections::BTreeMap<String, String>,
    /// packwiz instances: sync the pack before every launch.
    pub update_pack_on_launch: bool,
}

impl From<&Instance> for InstanceSettingsDto {
    fn from(instance: &Instance) -> Self {
        let settings = &instance.settings;
        Self {
            name: instance.name.to_string(),
            default_account_id: instance.default_account.map(|id| id.as_uuid().to_string()),
            java: match &settings.java {
                JavaSelection::Managed => JavaChoiceDto {
                    kind: JavaKindDto::Managed,
                    path: None,
                },
                JavaSelection::Custom(path) => JavaChoiceDto {
                    kind: JavaKindDto::Custom,
                    path: Some(path.display().to_string()),
                },
            },
            min_memory_mb: settings.memory.min_mb(),
            max_memory_mb: settings.memory.max_mb(),
            jvm_args: settings.jvm_args.as_slice().to_vec(),
            window: settings.window.map(|window| WindowSizeDto {
                width: window.width(),
                height: window.height(),
            }),
            env: settings
                .env
                .iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
            update_pack_on_launch: settings.update_pack_on_launch,
        }
    }
}

/// A pack file the player has to download by hand from its CurseForge page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManualFileDto {
    pub name: String,
    pub filename: String,
    pub page_url: String,
}

impl From<&ManualFile> for ManualFileDto {
    fn from(file: &ManualFile) -> Self {
        Self {
            name: file.name.clone(),
            filename: file.filename.clone(),
            page_url: file.page_url(),
        }
    }
}

/// A packwiz pack installed into a new instance; `pending` files still have
/// to be downloaded by hand before the pack is complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PackwizInstallDto {
    pub instance: InstanceDto,
    pub files: PackFilesDto,
    pub install: InstallSummaryDto,
    pub pending: Vec<ManualFileDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PackwizUpdateDto {
    pub instance: InstanceDto,
    pub files: PackFilesDto,
    pub pending: Vec<ManualFileDto>,
}

/// Manual downloads found in the Downloads folder, and those still missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManualDownloadsDto {
    pub resolved: Vec<ManualFileDto>,
    pub remaining: Vec<ManualFileDto>,
}

/// One entry of an instance's multiplayer server list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerDto {
    pub name: String,
    /// As the player typed it: `host`, `host:port` or `[ipv6]:port`.
    pub address: String,
    /// `null` asks in game; otherwise the remembered resource-pack choice.
    pub accept_textures: Option<bool>,
    /// The icon the game cached, as a `data:image/png` URL.
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerListDto {
    pub servers: Vec<ServerDto>,
}

impl From<&ServerList> for ServerListDto {
    fn from(list: &ServerList) -> Self {
        Self {
            servers: list
                .entries()
                .iter()
                .map(|entry| ServerDto {
                    name: entry.name.clone(),
                    address: entry.address.to_string(),
                    accept_textures: entry.accept_textures,
                    icon: entry
                        .icon
                        .as_ref()
                        .filter(|icon| {
                            icon.chars()
                                .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c))
                        })
                        .map(|icon| format!("data:image/png;base64,{icon}")),
                })
                .collect(),
        }
    }
}

/// A server the player saved in the launcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SavedServerDto {
    pub id: String,
    pub name: String,
    pub address: String,
    /// The instance Play uses; `null` until the player picks one.
    pub instance_id: Option<String>,
}

impl From<&launcher_core::domain::saved_server::SavedServer> for SavedServerDto {
    fn from(server: &launcher_core::domain::saved_server::SavedServer) -> Self {
        Self {
            id: server.id.as_uuid().to_string(),
            name: server.name.as_str().to_owned(),
            address: server.address.to_string(),
            instance_id: server.instance.as_ref().map(|id| id.as_str().to_owned()),
        }
    }
}

/// A saved server as the player typed it; `id` is `null` for a new one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SavedServerFormDto {
    pub id: Option<String>,
    pub name: String,
    pub address: String,
    pub instance_id: Option<String>,
}

/// What the player typed for a server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerFormDto {
    pub name: String,
    pub address: String,
    pub accept_textures: Option<bool>,
}

/// One change to a server list; positions are zero-based.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServerEditDto {
    Add { form: ServerFormDto },
    Update { index: u32, form: ServerFormDto },
    Remove { index: u32 },
    Move { from: u32, to: u32 },
}
