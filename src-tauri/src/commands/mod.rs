//! Thin `#[tauri::command]` functions.
//!
//! Each command validates its input, calls exactly one use case, and maps
//! the result or error to a DTO. No business logic lives here. Every command
//! is registered in `crate::bindings::commands` for the TypeScript map.

pub mod accounts;
pub mod app;
pub mod demo;
pub mod instance_management;
pub mod instances;
pub mod modpacks;
pub mod packwiz;
pub mod server;
pub mod versions;

use launcher_core::domain::account::AccountId;
use launcher_core::domain::instance::InstanceId;

use crate::error::AppError;

pub(crate) fn parse_instance_id(raw: &str) -> Result<InstanceId, AppError> {
    InstanceId::parse(raw).map_err(|e| AppError::invalid_input(e.to_string()))
}

pub(crate) fn parse_account_id(raw: &str) -> Result<AccountId, AppError> {
    uuid::Uuid::parse_str(raw)
        .map(AccountId::from_uuid)
        .map_err(|e| AppError::invalid_input(e.to_string()))
}
