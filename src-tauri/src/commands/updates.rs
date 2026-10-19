use launcher_core::application::check_app_update::check_app_update as check;
use launcher_core::application::install_app_update::install_app_update as install;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::dto::{AppUpdateStatusDto, ProgressEvent};
use crate::error::AppError;
use crate::progress::ChannelReporter;
use crate::state::AppState;
use crate::updater::TauriAppUpdater;

/// Asks the release source whether a newer launcher is available.
#[tauri::command]
pub async fn check_app_update(
    updater: State<'_, TauriAppUpdater>,
) -> Result<AppUpdateStatusDto, AppError> {
    Ok(check(updater.inner()).await?.into())
}

/// Downloads, verifies and installs `version` (the one the player accepted),
/// then restarts the launcher. Refused while a game installs or runs.
#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    state: State<'_, AppState>,
    updater: State<'_, TauriAppUpdater>,
    operation_id: String,
    version: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<(), AppError> {
    let token = state.operations.start(&operation_id)?;
    let result = install(
        &version,
        updater.inner(),
        &state.games,
        &ChannelReporter::new(on_progress),
        &token,
    )
    .await;
    state.operations.finish(&operation_id);
    result?;
    // Windows exits inside the installer; macOS and Linux start the new version here.
    app.restart()
}
