use tauri::AppHandle;

use crate::dto::AppInfo;

/// Product name and version from the Tauri package info.
#[tauri::command]
pub fn get_app_info(app: AppHandle) -> AppInfo {
    let info = app.package_info();
    AppInfo {
        name: info.name.clone(),
        version: info.version.to_string(),
    }
}
