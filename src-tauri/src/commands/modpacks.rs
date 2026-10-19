//! Modrinth modpacks: search, details, install, import, update and export.
//!
//! Installing and updating run the pack sync and then the regular instance
//! install (game, loader, Java) as one cancellable operation with one progress
//! channel, so the UI follows a single job. File pickers open from Rust, so
//! the webview never gets file-system or dialog permissions.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use launcher_core::application::export_mrpack::{
    ExportMrpackDeps, ExportMrpackRequest, export_mrpack as export,
};
use launcher_core::application::get_modpack_details::get_modpack_details as details;
use launcher_core::application::install_instance::install_instance as install;
use launcher_core::application::install_modpack::{
    InstallModpackDeps, InstallModpackRequest, PackInput, install_modpack as install_pack,
};
use launcher_core::application::prepare_cinepol_instance::cinepol_server_entry;
use launcher_core::application::search_modpacks::search_modpacks as search;
use launcher_core::application::update_modpack::{
    UpdateInput, UpdateModpackDeps, UpdateOutcome, update_modpack as update,
};
use launcher_core::domain::instance::{GameVersion, InstanceId, InstanceRepository};
use launcher_core::domain::loader::LoaderKind;
use launcher_core::domain::modpack::ModpackQuery;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio_util::sync::CancellationToken;

use super::parse_instance_id;
use crate::dto::{
    InstallSummaryDto, InstanceDto, ModpackDetailsDto, ModpackExportDto, ModpackInstallDto,
    ModpackPageDto, ModpackSummaryDto, ModpackUpdateDto, ModpackVersionDto, PackFilesDto,
    ProgressEvent,
};
use crate::error::{AppError, ErrorCode};
use crate::operations::Hold;
use crate::progress::ChannelReporter;
use crate::state::AppState;

const MRPACK_EXTENSION: &str = "mrpack";
/// Label of the file type filter in the native pickers.
const MRPACK_FILTER: &str = "Modrinth modpack";

/// One page of Modrinth modpacks; `page` is zero-based. `game_version` and
/// `loader` narrow the results when given.
#[tauri::command]
pub async fn search_modpacks(
    state: State<'_, AppState>,
    query: String,
    game_version: Option<String>,
    loader: Option<String>,
    page: u32,
    page_size: u32,
) -> Result<ModpackPageDto, AppError> {
    let game_version = game_version
        .filter(|raw| !raw.trim().is_empty())
        .map(|raw| GameVersion::parse(raw.trim()))
        .transpose()
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
    let loader = loader
        .map(|raw| {
            LoaderKind::from_id(&raw)
                .ok_or_else(|| AppError::invalid_input(format!("unsupported loader {raw:?}")))
        })
        .transpose()?;
    let query = ModpackQuery {
        text: query,
        game_version,
        loader,
        page,
        page_size,
    };
    Ok(ModpackPageDto::from(&search(&state.modrinth, query).await?))
}

/// A modpack's details and its versions, newest first.
#[tauri::command]
pub async fn get_modpack(
    state: State<'_, AppState>,
    project: String,
) -> Result<ModpackDetailsDto, AppError> {
    let found = details(&state.modrinth, project.trim()).await?;
    Ok(ModpackDetailsDto {
        summary: ModpackSummaryDto::from(&found.project),
        versions: found.versions.iter().map(ModpackVersionDto::from).collect(),
    })
}

/// Installs a Modrinth modpack version into a new instance, then the game and
/// its loader. Cancellable with `cancel_operation`.
#[tauri::command]
pub async fn install_modpack(
    state: State<'_, AppState>,
    operation_id: String,
    project: String,
    version: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<ModpackInstallDto, AppError> {
    let token = state.operations.start(&operation_id)?;
    let pack = PackInput::Catalog {
        project: project.trim(),
        version: version.trim(),
    };
    let result = install_new(&state, pack, &token, on_progress).await;
    state.operations.finish(&operation_id);
    result
}

/// Asks for a `.mrpack` file and installs it into a new instance, then the
/// game and its loader. Resolves `null` when the player closes the picker.
#[tauri::command]
pub async fn import_mrpack(
    app: AppHandle,
    state: State<'_, AppState>,
    operation_id: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<Option<ModpackInstallDto>, AppError> {
    // Registered before the picker opens, so the UI's Cancel works from the start.
    let token = state.operations.start(&operation_id)?;
    let result = match pick_mrpack(&app).await {
        Ok(Some(path)) if !token.is_cancelled() => {
            install_new(&state, PackInput::File(&path), &token, on_progress)
                .await
                .map(Some)
        }
        Ok(_) => Ok(None),
        Err(error) => Err(error),
    };
    state.operations.finish(&operation_id);
    result
}

/// Updates a Modrinth modpack instance to its newest version, keeping the
/// player's changes, then installs the game and loader it needs.
#[tauri::command]
pub async fn update_modpack(
    state: State<'_, AppState>,
    operation_id: String,
    instance_id: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<ModpackUpdateDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let token = state.operations.start(&operation_id)?;
    let result = update_held(&state, &id, &token, on_progress).await;
    state.operations.finish(&operation_id);
    result
}

/// Asks where to save and exports the instance as a `.mrpack`. Resolves
/// `null` when the player closes the picker.
#[tauri::command]
pub async fn export_mrpack(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Option<ModpackExportDto>, AppError> {
    let id = parse_instance_id(&instance_id)?;
    // Held like a running game: no export while the game, an install or an
    // update changes the folder, and none of those while the export reads it.
    let _claim = hold(&state, &id, &CancellationToken::new())?;
    let instance = state.instances.get(&id).await?;
    let Some(destination) = pick_destination(&app, &instance.name.to_string()).await? else {
        return Ok(None);
    };
    let request = ExportMrpackRequest {
        instance: &id,
        destination: &destination,
        version: None,
    };
    let deps = ExportMrpackDeps {
        instances: &state.instances,
        source: &state.modrinth,
        archive: &state.mrpack,
        files: &state.pack_files,
        paths: &state.paths,
    };
    let summary = export(request, &deps).await?;
    Ok(Some(ModpackExportDto::new(&destination, &summary)))
}

async fn install_new(
    state: &AppState,
    pack: PackInput<'_>,
    token: &CancellationToken,
    on_progress: Channel<ProgressEvent>,
) -> Result<ModpackInstallDto, AppError> {
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
    let request = InstallModpackRequest {
        pack,
        name: None,
        seed_server: Some(cinepol_server_entry(&state.current_config())),
        claim: Some(&claim),
    };
    let deps = InstallModpackDeps {
        instances: &state.instances,
        servers: &state.servers,
        source: &state.modrinth,
        pack: state.pack_sync(),
    };
    let installed = install_pack(request, &deps, &reporter, token).await?;
    let id = &installed.instance.id;
    let summary = install(id, &state.install_deps(), &reporter, token).await?;
    Ok(ModpackInstallDto {
        instance: InstanceDto::from(&installed.instance),
        files: PackFilesDto::from(&installed.files),
        install: InstallSummaryDto::from(&summary),
    })
}

async fn update_held(
    state: &AppState,
    id: &InstanceId,
    token: &CancellationToken,
    on_progress: Channel<ProgressEvent>,
) -> Result<ModpackUpdateDto, AppError> {
    // No update while the game runs, and kill_instance stops the update.
    let _claim = hold(state, id, token)?;
    let reporter = ChannelReporter::new(on_progress);
    let deps = UpdateModpackDeps {
        instances: &state.instances,
        source: &state.modrinth,
        pack: state.pack_sync(),
    };
    let outcome = update(
        id,
        UpdateInput::Catalog { version: None },
        &deps,
        &reporter,
        token,
    )
    .await?;
    match outcome {
        UpdateOutcome::UpToDate { .. } => Ok(ModpackUpdateDto {
            up_to_date: true,
            instance: None,
            files: None,
        }),
        UpdateOutcome::Updated { instance, files } => {
            install(id, &state.install_deps(), &reporter, token).await?;
            Ok(ModpackUpdateDto {
                up_to_date: false,
                instance: Some(InstanceDto::from(instance.as_ref())),
                files: Some(PackFilesDto::from(&files)),
            })
        }
    }
}

/// Claims the instance as busy, like a running game.
pub(crate) fn hold<'a>(
    state: &'a AppState,
    id: &InstanceId,
    token: &CancellationToken,
) -> Result<Hold<'a>, AppError> {
    state.games.hold(id.as_str(), token.clone()).map_err(|_| {
        AppError::new(
            ErrorCode::AlreadyRunning,
            format!("{id} is busy (running, installing, updating or exporting)"),
        )
    })
}

async fn pick_mrpack(app: &AppHandle) -> Result<Option<PathBuf>, AppError> {
    let dialog = app
        .dialog()
        .file()
        .add_filter(MRPACK_FILTER, &[MRPACK_EXTENSION]);
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

async fn pick_destination(app: &AppHandle, name: &str) -> Result<Option<PathBuf>, AppError> {
    let dialog = app
        .dialog()
        .file()
        .add_filter(MRPACK_FILTER, &[MRPACK_EXTENSION])
        .set_file_name(format!("{}.{MRPACK_EXTENSION}", file_stem(name)));
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))?;
    let Some(path) = picked else {
        return Ok(None);
    };
    let mut path = path
        .into_path()
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))?;
    if path.extension().is_none() {
        path.set_extension(MRPACK_EXTENSION);
    }
    Ok(Some(path))
}

/// A file name suggestion from an instance name: no path separators or
/// characters Windows refuses.
fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim().trim_matches('.').to_owned();
    if stem.is_empty() {
        "modpack".to_owned()
    } else {
        stem
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_file_names_drop_separators_and_reserved_characters() {
        assert_eq!(file_stem("Fabulously Optimized"), "Fabulously Optimized");
        assert_eq!(file_stem("a/b\\c:d*?"), "a_b_c_d__");
        assert_eq!(file_stem(" .. "), "modpack");
    }
}
