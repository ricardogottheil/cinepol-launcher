use launcher_core::application::install_instance::InstallDeps;
use launcher_core::application::launch_instance::LaunchDeps;
use launcher_core::application::load_server_config::load_server_config;
use launcher_core::application::play_on_cinepol::play_on_cinepol as play;
use launcher_core::domain::server_list::ServerAddress;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use crate::dto::{LaunchOutcomeDto, LogLineDto, ProgressEvent, ServerConfigDto};
use crate::error::{AppError, ErrorCode};
use crate::events::GAME_EXITED;
use crate::logs::ChannelLogSink;
use crate::progress::ChannelReporter;
use crate::state::AppState;

/// The CINEPOL server config: remote when reachable, bundled otherwise.
#[tauri::command]
pub async fn get_server_config(state: State<'_, AppState>) -> Result<ServerConfigDto, AppError> {
    let loaded = load_server_config(&state.config_source, &state.bundled_config).await;
    state.remember_config(loaded.config.clone());
    Ok(ServerConfigDto::new(&loaded))
}

/// Prepares the managed instance, installs it and joins `server` (a community
/// server from the config; the main one when `None`, none when the config
/// lists no server). Install
/// progress, then a `launching` step, arrive through `on_progress`; the game
/// output through `on_log`. `cancel_operation(operation_id)` stops either.
#[tauri::command]
pub async fn play_on_cinepol(
    app: AppHandle,
    state: State<'_, AppState>,
    operation_id: String,
    on_progress: Channel<ProgressEvent>,
    on_log: Channel<LogLineDto>,
    server: Option<String>,
) -> Result<LaunchOutcomeDto, AppError> {
    let server = server
        .as_deref()
        .map(ServerAddress::parse)
        .transpose()
        .map_err(|error| AppError::new(ErrorCode::InvalidInput, error.to_string()))?;
    let _session = state.cinepol_session.try_enter().ok_or_else(|| {
        AppError::new(
            ErrorCode::AlreadyRunning,
            "a Play on CINEPOL session is already running",
        )
    })?;
    let token = state.operations.start(&operation_id)?;
    let config = state.current_config();
    let install = InstallDeps {
        instances: &state.instances,
        versions: &state.versions,
        downloader: &state.downloader,
        java: &state.java,
        paths: &state.paths,
        platform: &state.platform,
        loaders: &state.loaders,
    };
    let launch = LaunchDeps {
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
    // Claimed in `state.games` too, so kill_instance and the instances page
    // see this session.
    let result = play(
        &config,
        server.as_ref(),
        &state.servers,
        &state.games,
        &install,
        &launch,
        &ChannelReporter::new(on_progress),
        ChannelLogSink::new(on_log),
        &token,
        token.clone(),
    )
    .await;
    state.operations.finish(&operation_id);
    let session = result?;
    let outcome = LaunchOutcomeDto::new(session.instance.id.to_string(), &session.outcome);
    let _ = app.emit(GAME_EXITED, &outcome);
    Ok(outcome)
}
