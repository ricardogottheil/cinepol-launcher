//! packwiz packs: install from an https URL or a local `pack.toml` picked
//! with the native file picker, update, and finish with files the player
//! downloads by hand (CurseForge files the launcher cannot fetch itself).
//!
//! The webview only ever sends https URLs; local packs come from the picker,
//! so the UI cannot point the launcher at arbitrary paths. Installs and
//! updates chain the regular instance install (game, loader, Java) as one
//! cancellable operation, like Modrinth modpacks.

use std::sync::{Mutex, PoisonError};

use launcher_core::application::install_instance::install_instance as install;
use launcher_core::application::install_packwiz::{
    InstallPackwizDeps, InstallPackwizRequest, install_packwiz as install_pack,
};
use launcher_core::application::prepare_cinepol_instance::cinepol_server_entry;
use launcher_core::application::resolve_manual_downloads::{
    ResolveManualDownloadsDeps, resolve_manual_downloads as resolve,
};
use launcher_core::application::update_packwiz::{UpdatePackwizDeps, update_packwiz as update};
use launcher_core::domain::instance::InstanceId;
use launcher_core::domain::modpack::PackLocation;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio_util::sync::CancellationToken;

use super::modpacks::hold;
use super::parse_instance_id;
use crate::dto::{
    InstallSummaryDto, InstanceDto, ManualDownloadsDto, ManualFileDto, PackFilesDto,
    PackwizInstallDto, PackwizUpdateDto, ProgressEvent,
};
use crate::error::{AppError, ErrorCode};
use crate::operations::Hold;
use crate::progress::ChannelReporter;
use crate::state::AppState;

/// Label of the file type filter in the native picker.
const PACK_FILTER: &str = "packwiz pack";

/// Installs the packwiz pack at an https `pack.toml` URL into a new instance,
/// then the game and its loader. Cancellable with `cancel_operation`.
#[tauri::command]
pub async fn install_packwiz(
    state: State<'_, AppState>,
    operation_id: String,
    url: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<PackwizInstallDto, AppError> {
    let location = match PackLocation::parse(&url) {
        Ok(location @ PackLocation::Url(_)) => location,
        _ => return Err(AppError::invalid_input("enter an https URL of a pack.toml")),
    };
    let token = state.operations.start(&operation_id)?;
    let result = install_new(&state, &location, &token, on_progress).await;
    state.operations.finish(&operation_id);
    result
}

/// Asks for a local `pack.toml` and installs it. Resolves `null` when the
/// player closes the picker.
#[tauri::command]
pub async fn import_packwiz(
    app: AppHandle,
    state: State<'_, AppState>,
    operation_id: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<Option<PackwizInstallDto>, AppError> {
    // Registered before the picker opens, so the UI's Cancel works from the start.
    let token = state.operations.start(&operation_id)?;
    let result = match pick_pack(&app).await {
        Ok(Some(path)) if !token.is_cancelled() => {
            install_new(&state, &PackLocation::Local(path), &token, on_progress)
                .await
                .map(Some)
        }
        Ok(_) => Ok(None),
        Err(error) => Err(error),
    };
    state.operations.finish(&operation_id);
    result
}

/// Syncs a packwiz instance with its pack, keeping the player's changes, then
/// installs the game and loader it needs.
#[tauri::command]
pub async fn update_packwiz(
    state: State<'_, AppState>,
    operation_id: String,
    instance_id: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<PackwizUpdateDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let token = state.operations.start(&operation_id)?;
    let result = update_held(&state, &id, &token, on_progress).await;
    state.operations.finish(&operation_id);
    result
}

/// Installs the pending manual downloads found in the Downloads folder;
/// the UI polls this while the list is open.
#[tauri::command]
pub async fn check_manual_downloads(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<ManualDownloadsDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let downloads = app.path().download_dir()?;
    let deps = ResolveManualDownloadsDeps {
        files: &state.pack_files,
        manifests: &state.pack_manifests,
        paths: &state.paths,
    };
    let found = resolve(&id, &downloads, &deps).await?;
    Ok(ManualDownloadsDto {
        resolved: found.resolved.iter().map(ManualFileDto::from).collect(),
        remaining: found.remaining.iter().map(ManualFileDto::from).collect(),
    })
}

async fn install_new(
    state: &AppState,
    location: &PackLocation,
    token: &CancellationToken,
    on_progress: Channel<ProgressEvent>,
) -> Result<PackwizInstallDto, AppError> {
    let reporter = ChannelReporter::new(on_progress);
    // The new instance is held from the moment it exists until the game is installed.
    let held: Mutex<Option<Hold<'_>>> = Mutex::new(None);
    let claim = |id: &InstanceId| match hold(state, id, token) {
        Ok(guard) => {
            *held.lock().unwrap_or_else(PoisonError::into_inner) = Some(guard);
            true
        }
        Err(_) => false,
    };
    let request = InstallPackwizRequest {
        pack: location,
        name: None,
        seed_server: Some(cinepol_server_entry(&state.current_config())),
        claim: Some(&claim),
    };
    let deps = InstallPackwizDeps {
        instances: &state.instances,
        servers: &state.servers,
        source: &state.packwiz,
        curseforge: &state.curseforge,
        files: state.pack_sync().file_sync(),
    };
    let installed = install_pack(request, &deps, &reporter, token).await?;
    let id = &installed.instance.id;
    let summary = install(id, &state.install_deps(), &reporter, token).await?;
    Ok(PackwizInstallDto {
        instance: InstanceDto::from(&installed.instance),
        files: PackFilesDto::from(&installed.files),
        install: InstallSummaryDto::from(&summary),
        pending: installed.pending.iter().map(ManualFileDto::from).collect(),
    })
}

async fn update_held(
    state: &AppState,
    id: &InstanceId,
    token: &CancellationToken,
    on_progress: Channel<ProgressEvent>,
) -> Result<PackwizUpdateDto, AppError> {
    // No update while the game runs, and kill_instance stops the update.
    let _claim = hold(state, id, token)?;
    let reporter = ChannelReporter::new(on_progress);
    let deps = UpdatePackwizDeps {
        instances: &state.instances,
        source: &state.packwiz,
        curseforge: &state.curseforge,
        files: state.pack_sync().file_sync(),
    };
    let updated = update(id, &deps, &reporter, token).await?;
    install(id, &state.install_deps(), &reporter, token).await?;
    Ok(PackwizUpdateDto {
        instance: InstanceDto::from(&updated.instance),
        files: PackFilesDto::from(&updated.files),
        pending: updated.pending.iter().map(ManualFileDto::from).collect(),
    })
}

async fn pick_pack(app: &AppHandle) -> Result<Option<std::path::PathBuf>, AppError> {
    let dialog = app.dialog().file().add_filter(PACK_FILTER, &["toml"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))?;
    picked
        .map(|path| {
            path.into_path()
                .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))
        })
        .transpose()
}
