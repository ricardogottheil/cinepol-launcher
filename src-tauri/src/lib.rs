//! Cinepol Launcher desktop shell.
//!
//! This crate is a driving adapter only: it builds the Tauri app, wires
//! infrastructure adapters into use cases (`state`), and exposes thin
//! commands (`commands`). Business logic lives in `cinepol-launcher-core`.

pub mod bindings;
mod commands;
mod dto;
mod error;
mod events;
mod logs;
mod operations;
mod progress;
mod state;

use tauri::Manager;

/// Name of the launcher's folder inside the OS data directory.
const DATA_DIR_NAME: &str = "CinepolLauncher";

/// Builds and runs the Tauri application.
pub fn run() -> Result<(), tauri::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().data_dir()?.join(DATA_DIR_NAME);
            let version = app.package_info().version.to_string();
            let state = state::AppState::new(data_dir, &version).map_err(|error| error.message)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::demo::run_progress_demo,
            commands::demo::cancel_operation,
            commands::accounts::list_accounts,
            commands::accounts::create_offline_account,
            commands::accounts::set_active_account,
            commands::versions::list_game_versions,
            commands::instances::list_instances,
            commands::instances::create_instance,
            commands::instances::install_instance,
            commands::instances::launch_instance,
            commands::instances::kill_instance,
            commands::server::get_server_config,
            commands::server::get_server_status,
            commands::server::play_on_cinepol,
            commands::server::get_settings,
            commands::server::set_server_config_url,
        ])
        .run(tauri::generate_context!())
}
