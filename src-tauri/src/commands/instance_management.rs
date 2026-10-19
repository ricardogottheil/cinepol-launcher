//! Deleting, duplicating and editing instances.
//!
//! Delete and duplicate hold the instance like a running game, so neither
//! runs while it plays, installs, updates or exports. Settings can change
//! while the game runs; they apply on the next launch. A custom Java path is
//! accepted only when it was picked with the native file picker in this
//! session or is the one already saved, and the core refuses JVM arguments
//! and environment variables that load code (Java agents, `-XX:OnError`,
//! `JAVA_TOOL_OPTIONS`, `LD_PRELOAD`, `DYLD_*`), so the webview cannot make
//! the launcher run an arbitrary program.

use std::path::PathBuf;

use launcher_core::application::delete_instance::delete_instance as delete;
use launcher_core::application::duplicate_instance::duplicate_instance as duplicate;
use launcher_core::application::update_instance_settings::{
    InstanceSettingsChange, JavaChoice, UpdateInstanceSettingsDeps,
    update_instance_settings as update,
};
use launcher_core::domain::instance::{InstanceRepository, JavaSelection};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio_util::sync::CancellationToken;

use super::{parse_account_id, parse_instance_id};
use crate::dto::{InstanceDto, InstanceSettingsDto, JavaKindDto};
use crate::error::{AppError, ErrorCode};
use crate::state::AppState;

/// Deletes an instance and its whole folder, worlds included.
#[tauri::command]
pub async fn delete_instance(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), AppError> {
    let id = parse_instance_id(&instance_id)?;
    let _claim = hold(&state, &id)?;
    delete(&id, &state.instances).await?;
    Ok(())
}

/// Copies an instance, worlds included, under `name`.
#[tauri::command]
pub async fn duplicate_instance(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<InstanceDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    // Nothing writes to the source folder while it is copied.
    let _claim = hold(&state, &id)?;
    let copy = duplicate(&id, &name, &state.instances, &state.fs, &state.paths).await?;
    Ok(InstanceDto::from(&copy))
}

#[tauri::command]
pub async fn get_instance_settings(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<InstanceSettingsDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    Ok(InstanceSettingsDto::from(&state.instances.get(&id).await?))
}

/// Saves every editable value at once; nothing changes when one is invalid.
#[tauri::command]
pub async fn update_instance_settings(
    state: State<'_, AppState>,
    instance_id: String,
    settings: InstanceSettingsDto,
) -> Result<InstanceSettingsDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let current = state.instances.get(&id).await?;
    let java = match settings.java.kind {
        JavaKindDto::Managed => JavaChoice::Managed,
        JavaKindDto::Custom => {
            let path = PathBuf::from(settings.java.path.as_deref().unwrap_or_default());
            let saved = current.settings.java == JavaSelection::Custom(path.clone());
            if !saved && !state.was_picked_java(&path) {
                return Err(AppError::invalid_input(
                    "choose the Java executable with the file picker",
                ));
            }
            JavaChoice::Custom(path)
        }
    };
    let default_account = settings
        .default_account_id
        .as_deref()
        .map(parse_account_id)
        .transpose()?;
    let change = InstanceSettingsChange {
        name: settings.name,
        default_account,
        java,
        min_memory_mb: settings.min_memory_mb,
        max_memory_mb: settings.max_memory_mb,
        jvm_args: settings.jvm_args,
        window: settings.window.map(|window| (window.width, window.height)),
        env: settings.env,
        update_pack_on_launch: settings.update_pack_on_launch,
    };
    let deps = UpdateInstanceSettingsDeps {
        instances: &state.instances,
        accounts: &state.accounts,
        java: &state.java_probe,
    };
    Ok(InstanceSettingsDto::from(
        &update(&id, change, &deps).await?,
    ))
}

/// Asks for a Java executable; resolves `null` when the picker is closed.
/// The path is remembered so `update_instance_settings` accepts it.
#[tauri::command]
pub async fn pick_java_executable(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, AppError> {
    let dialog = app.dialog().file();
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))?;
    state.remember_picked_java(path.clone());
    Ok(Some(path.display().to_string()))
}

fn hold<'a>(
    state: &'a AppState,
    id: &launcher_core::domain::instance::InstanceId,
) -> Result<crate::operations::Hold<'a>, AppError> {
    state
        .games
        .hold(id.as_str(), CancellationToken::new())
        .map_err(|_| {
            AppError::new(
                ErrorCode::AlreadyRunning,
                format!("{id} is busy (running, installing, updating or exporting)"),
            )
        })
}
