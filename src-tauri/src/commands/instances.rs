use launcher_core::application::create_instance::{NewInstance, create_instance as create};
use launcher_core::application::install_instance::install_instance as install;
use launcher_core::application::launch_instance::{
    LaunchDeps, LaunchRequest, launch_instance as launch,
};
use launcher_core::application::list_instances::list_instances as list;
use launcher_core::application::prepare_cinepol_instance::cinepol_server_entry;
use launcher_core::domain::instance::InstanceOrigin;
use launcher_core::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use super::parse_instance_id;
use crate::dto::{InstallSummaryDto, InstanceDto, LaunchOutcomeDto, LogLineDto, ProgressEvent};
use crate::error::{AppError, ErrorCode};
use crate::events::GAME_EXITED;
use crate::logs::ChannelLogSink;
use crate::progress::ChannelReporter;
use crate::state::AppState;

#[tauri::command]
pub async fn list_instances(state: State<'_, AppState>) -> Result<Vec<InstanceDto>, AppError> {
    Ok(list(&state.instances)
        .await?
        .iter()
        .map(InstanceDto::from)
        .collect())
}

/// Creates an instance with CINEPOL first in its server list. `loader` is
/// `vanilla`, `fabric`, `quilt`, `forge` or `neoforge`; all but vanilla need a
/// `loader_version`.
#[tauri::command]
pub async fn create_instance(
    state: State<'_, AppState>,
    name: String,
    game_version: String,
    loader: String,
    loader_version: Option<String>,
) -> Result<InstanceDto, AppError> {
    let loader = parse_loader(&loader, loader_version.as_deref())?;
    let request = NewInstance {
        name: &name,
        game_version: &game_version,
        loader,
        origin: InstanceOrigin::Custom,
        seed_server: Some(cinepol_server_entry(&state.current_config())),
    };
    Ok(InstanceDto::from(
        &create(request, &state.instances, &state.servers).await?,
    ))
}

/// Downloads everything the instance needs; cancellable with `cancel_operation`.
#[tauri::command]
pub async fn install_instance(
    state: State<'_, AppState>,
    operation_id: String,
    instance_id: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<InstallSummaryDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let token = state.operations.start(&operation_id)?;
    // Claimed as a running game too: no install over a running game, no
    // launch over an install, and kill_instance stops the install.
    let Ok(_claim) = state.games.hold(id.as_str(), token.clone()) else {
        state.operations.finish(&operation_id);
        return Err(AppError::new(
            ErrorCode::AlreadyRunning,
            format!("{id} is already running"),
        ));
    };
    let result = install(
        &id,
        &state.install_deps(),
        &ChannelReporter::new(on_progress),
        &token,
    )
    .await;
    state.operations.finish(&operation_id);
    Ok(InstallSummaryDto::from(&result?))
}

/// Starts the game and resolves when it exits; output streams through `on_log`.
#[tauri::command]
pub async fn launch_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    on_log: Channel<LogLineDto>,
) -> Result<LaunchOutcomeDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let kill = state.games.start(id.as_str()).map_err(|_| {
        AppError::new(
            ErrorCode::AlreadyRunning,
            format!("{id} is already running"),
        )
    })?;
    let deps = LaunchDeps {
        instances: &state.instances,
        accounts: &state.accounts,
        versions: &state.versions,
        java: &state.java,
        natives: &state.natives,
        runner: &state.runner,
        clock: &state.clock,
        fs: &state.fs,
        paths: &state.paths,
        platform: &state.platform,
        sessions: &state.sessions(),
        launcher_name: crate::state::LAUNCHER_NAME,
        launcher_version: &state.launcher_version,
    };
    let request = LaunchRequest {
        instance: id.clone(),
        account: None,
        join_server: None,
    };
    let result = launch(&request, &deps, ChannelLogSink::new(on_log), kill).await;
    state.games.finish(id.as_str());
    let outcome = LaunchOutcomeDto::new(id.to_string(), &result?);
    // Other windows and the tray learn about the exit without holding the channel.
    let _ = app.emit(GAME_EXITED, &outcome);
    Ok(outcome)
}

/// Stops a running game. Returns `false` when it is not running.
#[tauri::command]
pub fn kill_instance(state: State<'_, AppState>, instance_id: String) -> Result<bool, AppError> {
    let id = parse_instance_id(&instance_id)?;
    Ok(state.games.cancel(id.as_str()))
}

/// Validates a loader choice from the UI.
fn parse_loader(kind: &str, version: Option<&str>) -> Result<LoaderSpec, AppError> {
    let kind = LoaderKind::from_id(kind)
        .ok_or_else(|| AppError::invalid_input(format!("unsupported loader {kind:?}")))?;
    let version = version
        .map(|raw| LoaderVersion::parse(raw.trim()))
        .transpose()
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
    LoaderSpec::new(kind, version).map_err(|error| AppError::invalid_input(error.to_string()))
}
