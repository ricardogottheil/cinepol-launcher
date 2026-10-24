//! Single mods from Modrinth: search filtered by the instance's game version
//! and loader, an install preview with its dependencies, install, the
//! installed list, enable/disable, removal and updates.
//!
//! Commands that change `mods/` hold the instance like a running game, so
//! two of them never write the same folder (or `mods.json`) at once, and none
//! runs while the game does.

use launcher_core::application::check_mod_updates::{
    CheckModUpdatesDeps, check_mod_updates as check_updates,
};
use launcher_core::application::install_mod::{InstallModDeps, install_mod as install};
use launcher_core::application::list_instance_mods::{
    ListInstanceModsDeps, list_instance_mods as list,
};
use launcher_core::application::preview_mod_install::{
    PreviewModInstallDeps, preview_mod_install as preview,
};
use launcher_core::application::remove_mod::{RemoveModDeps, remove_mod as remove};
use launcher_core::application::search_mods::search_mods as search;
use launcher_core::application::set_mod_enabled::{
    SetModEnabledDeps, set_mod_enabled as set_enabled,
};
use launcher_core::application::update_mod::update_mod as update;
use launcher_core::domain::instance::InstanceId;
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use super::modpacks::hold;
use super::parse_instance_id;
use crate::dto::{
    InstalledModDto, ModEntryDto, ModInstallPreviewDto, ModPageDto, ModUpdateDto, ProgressEvent,
};
use crate::error::AppError;
use crate::progress::ChannelReporter;
use crate::state::AppState;

/// One page of mods that run on the instance; `page` is zero-based.
#[tauri::command]
pub async fn search_mods(
    state: State<'_, AppState>,
    instance_id: String,
    query: String,
    page: u32,
    page_size: u32,
) -> Result<ModPageDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let page = search(
        &state.instances,
        &state.modrinth,
        &id,
        query,
        page,
        page_size,
    )
    .await?;
    Ok(ModPageDto::from(&page))
}

/// What installing `project` would write, so the player sees the
/// dependencies before confirming.
#[tauri::command]
pub async fn preview_mod_install(
    state: State<'_, AppState>,
    instance_id: String,
    project: String,
) -> Result<ModInstallPreviewDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let deps = PreviewModInstallDeps {
        instances: &state.instances,
        source: &state.modrinth,
        files: &state.pack_files,
        registry: &state.mod_registry,
        paths: &state.paths,
    };
    let plan = preview(&deps, &id, project.trim()).await?;
    Ok(ModInstallPreviewDto::from(&plan))
}

/// Installs `project` and its required dependencies. Cancellable with
/// `cancel_operation`.
#[tauri::command]
pub async fn install_mod(
    state: State<'_, AppState>,
    operation_id: String,
    instance_id: String,
    project: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<Vec<InstalledModDto>, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let token = state.operations.start(&operation_id)?;
    let result = write(&state, &id, project.trim(), false, &token, on_progress).await;
    state.operations.finish(&operation_id);
    result
}

/// Replaces `project` with its newest compatible version. Cancellable with
/// `cancel_operation`.
#[tauri::command]
pub async fn update_mod(
    state: State<'_, AppState>,
    operation_id: String,
    instance_id: String,
    project: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<Vec<InstalledModDto>, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let token = state.operations.start(&operation_id)?;
    let result = write(&state, &id, project.trim(), true, &token, on_progress).await;
    state.operations.finish(&operation_id);
    result
}

/// Every jar in the instance's `mods/` folder, sorted by title.
#[tauri::command]
pub async fn list_instance_mods(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<ModEntryDto>, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let deps = ListInstanceModsDeps {
        instances: &state.instances,
        source: &state.modrinth,
        files: &state.pack_files,
        registry: &state.mod_registry,
        paths: &state.paths,
    };
    let entries = list(&deps, &id).await?;
    Ok(ModEntryDto::list(&entries))
}

/// Turns the jar `filename` on or off.
#[tauri::command]
pub async fn set_mod_enabled(
    state: State<'_, AppState>,
    instance_id: String,
    filename: String,
    enabled: bool,
) -> Result<(), AppError> {
    let id = parse_instance_id(&instance_id)?;
    let _claim = hold(&state, &id, &CancellationToken::new())?;
    let deps = SetModEnabledDeps {
        instances: &state.instances,
        files: &state.pack_files,
        paths: &state.paths,
    };
    set_enabled(&deps, &id, &filename, enabled).await?;
    Ok(())
}

/// Removes the jar `filename`, and with `remove_orphans` the dependencies
/// nothing else needs any more. Resolves the file names removed.
#[tauri::command]
pub async fn remove_mod(
    state: State<'_, AppState>,
    instance_id: String,
    filename: String,
    remove_orphans: bool,
) -> Result<Vec<String>, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let _claim = hold(&state, &id, &CancellationToken::new())?;
    let deps = RemoveModDeps {
        instances: &state.instances,
        source: &state.modrinth,
        files: &state.pack_files,
        registry: &state.mod_registry,
        paths: &state.paths,
    };
    Ok(remove(&deps, &id, &filename, remove_orphans).await?)
}

/// The installed mods that have a newer compatible version.
#[tauri::command]
pub async fn check_mod_updates(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<ModUpdateDto>, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let deps = CheckModUpdatesDeps {
        instances: &state.instances,
        source: &state.modrinth,
        files: &state.pack_files,
        registry: &state.mod_registry,
        paths: &state.paths,
    };
    let updates = check_updates(&deps, &id).await?;
    Ok(updates.iter().map(ModUpdateDto::from).collect())
}

async fn write(
    state: &AppState,
    id: &InstanceId,
    project: &str,
    replace: bool,
    token: &CancellationToken,
    on_progress: Channel<ProgressEvent>,
) -> Result<Vec<InstalledModDto>, AppError> {
    // No change to mods/ while the game runs, and kill_instance stops it.
    let _claim = hold(state, id, token)?;
    let reporter = ChannelReporter::new(on_progress);
    let deps = InstallModDeps {
        instances: &state.instances,
        source: &state.modrinth,
        downloader: &state.downloader,
        fs: &state.fs,
        files: &state.pack_files,
        registry: &state.mod_registry,
        paths: &state.paths,
    };
    let written = if replace {
        update(&deps, id, project, &reporter, token).await?
    } else {
        install(&deps, id, project, &reporter, token).await?
    };
    Ok(written.iter().map(InstalledModDto::from).collect())
}
