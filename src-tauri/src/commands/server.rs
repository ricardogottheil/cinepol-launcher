use launcher_core::application::check_server_status::check_server_status;
use launcher_core::application::get_settings::get_settings as load_settings;
use launcher_core::application::install_instance::InstallDeps;
use launcher_core::application::launch_instance::LaunchDeps;
use launcher_core::application::load_server_config::load_server_config;
use launcher_core::application::play_on_cinepol::play_on_cinepol as play;
use launcher_core::application::update_settings::set_server_config_url as set_url;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use crate::dto::{
    LaunchOutcomeDto, LogLineDto, ProgressEvent, ServerConfigDto, ServerStatusDto, SettingsDto,
};
use crate::error::{AppError, ErrorCode};
use crate::events::GAME_EXITED;
use crate::logs::ChannelLogSink;
use crate::progress::ChannelReporter;
use crate::state::AppState;

/// The CINEPOL server config: remote when reachable, bundled otherwise.
#[tauri::command]
pub async fn get_server_config(state: State<'_, AppState>) -> Result<ServerConfigDto, AppError> {
    let loaded =
        load_server_config(&state.settings, &state.config_source, &state.bundled_config).await;
    state.remember_config(loaded.config.clone());
    Ok(ServerConfigDto::new(&loaded))
}

/// Pings the CINEPOL server (Server List Ping).
#[tauri::command]
pub async fn get_server_status(state: State<'_, AppState>) -> Result<ServerStatusDto, AppError> {
    let status = check_server_status(&state.pinger, &state.current_config()).await?;
    Ok(ServerStatusDto::from(&status))
}

/// Prepares the managed instance, installs it and joins the server. Install
/// progress, then a `launching` step, arrive through `on_progress`; the game
/// output through `on_log`. `cancel_operation(operation_id)` stops either.
#[tauri::command]
pub async fn play_on_cinepol(
    app: AppHandle,
    state: State<'_, AppState>,
    operation_id: String,
    on_progress: Channel<ProgressEvent>,
    on_log: Channel<LogLineDto>,
) -> Result<LaunchOutcomeDto, AppError> {
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
        launcher_name: crate::state::LAUNCHER_NAME,
        launcher_version: &state.launcher_version,
    };
    // Claimed in `state.games` too, so kill_instance and the instances page
    // see this session.
    let result = play(
        &config,
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

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<SettingsDto, AppError> {
    Ok(SettingsDto::from(&load_settings(&state.settings).await?))
}

/// `None` restores the default config URL.
#[tauri::command]
pub async fn set_server_config_url(
    state: State<'_, AppState>,
    url: Option<String>,
) -> Result<SettingsDto, AppError> {
    Ok(SettingsDto::from(
        &set_url(url.as_deref(), &state.settings).await?,
    ))
}
