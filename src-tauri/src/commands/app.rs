use tauri::{AppHandle, State};

use crate::dto::AppInfo;
use crate::error::{AppError, ErrorCode};
use crate::state::AppState;

/// Product name and version from the Tauri package info.
#[tauri::command]
pub fn get_app_info(app: AppHandle) -> AppInfo {
    let info = app.package_info();
    AppInfo {
        name: info.name.clone(),
        version: info.version.to_string(),
    }
}

/// Opens the folder with the launcher's log and each game's output in the
/// system's file manager, creating it when nothing was logged yet.
#[tauri::command]
pub fn open_logs_folder(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;

    let logs = state.paths.logs_dir();
    std::fs::create_dir_all(&logs)
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))?;
    app.opener()
        .open_path(logs.to_string_lossy(), None::<&str>)
        .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))
}
