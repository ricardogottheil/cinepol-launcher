//! Serializable DTOs exchanged with the frontend, mapped from domain types.
//!
//! Every DTO exported to TypeScript is registered in `crate::bindings`.

use launcher_core::domain::ports::ProgressUpdate;
use serde::Serialize;
use ts_rs::TS;

/// Application metadata shown in the UI (About, footer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// One progress step sent through a `Channel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number | null")]
    pub total: Option<u64>,
}

impl From<ProgressUpdate> for ProgressEvent {
    fn from(update: ProgressUpdate) -> Self {
        Self {
            done: update.done,
            total: update.total,
        }
    }
}
