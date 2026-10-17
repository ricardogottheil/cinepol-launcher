//! Serializable DTOs exchanged with the frontend, mapped from domain types.
//!
//! Every DTO exported to TypeScript is registered in `crate::bindings`.

use launcher_core::application::install_instance::InstallSummary;
use launcher_core::application::launch_instance::LaunchOutcome;
use launcher_core::domain::account::{Account, AccountKind};
use launcher_core::domain::instance::{Instance, InstanceOrigin};
use launcher_core::domain::launch::{LogStream, VersionSummary};
use launcher_core::domain::ports::{ProgressStage, ProgressUpdate};
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
}

impl From<ProgressUpdate> for ProgressEvent {
    fn from(update: ProgressUpdate) -> Self {
        Self {
            done: update.done,
            total: update.total,
            stage: update.stage.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AccountKindDto {
    Microsoft,
    Offline,
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
                AccountKind::Microsoft => AccountKindDto::Microsoft,
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
