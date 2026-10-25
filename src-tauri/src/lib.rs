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
mod redact;
mod state;
mod updater;

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
        // Masks tokens, auth codes and API keys before any line is written.
        .with_writer(redact::RedactingMakeWriter::new(std::io::stdout))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().data_dir()?.join(DATA_DIR_NAME);
            let version = app.package_info().version.to_string();
            let state = state::AppState::new(data_dir, &version).map_err(|error| error.message)?;
            app.manage(state);
            app.manage(updater::TauriAppUpdater::new(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::updates::check_app_update,
            commands::updates::install_app_update,
            commands::demo::run_progress_demo,
            commands::demo::cancel_operation,
            commands::accounts::list_accounts,
            commands::accounts::create_offline_account,
            commands::accounts::check_premium_name,
            commands::accounts::get_account_policy,
            commands::accounts::login_microsoft,
            commands::accounts::remove_account,
            commands::accounts::get_account_skin,
            commands::saved_servers::list_saved_servers,
            commands::saved_servers::save_server,
            commands::saved_servers::remove_saved_server,
            commands::saved_servers::ping_server,
            commands::accounts::change_account_skin,
            commands::accounts::list_account_capes,
            commands::accounts::set_account_cape,
            commands::accounts::set_active_account,
            commands::versions::list_game_versions,
            commands::versions::list_loader_versions,
            commands::instances::list_instances,
            commands::instances::create_instance,
            commands::instances::install_instance,
            commands::instances::launch_instance,
            commands::instances::kill_instance,
            commands::instances::open_crash_report,
            commands::instances::open_instance_folder,
            commands::instance_management::delete_instance,
            commands::instance_management::duplicate_instance,
            commands::instance_management::get_instance_settings,
            commands::instance_management::update_instance_settings,
            commands::instance_management::pick_java_executable,
            commands::modpacks::search_modpacks,
            commands::modpacks::get_modpack,
            commands::modpacks::install_modpack,
            commands::modpacks::import_mrpack,
            commands::modpacks::import_curseforge_zip,
            commands::modpacks::get_modpack_catalogs,
            commands::modpacks::update_modpack,
            commands::modpacks::export_mrpack,
            commands::mods::search_mods,
            commands::mods::preview_mod_install,
            commands::mods::install_mod,
            commands::mods::update_mod,
            commands::mods::list_instance_mods,
            commands::mods::set_mod_enabled,
            commands::mods::remove_mod,
            commands::mods::check_mod_updates,
            commands::packwiz::install_packwiz,
            commands::packwiz::import_packwiz,
            commands::packwiz::update_packwiz,
            commands::packwiz::check_manual_downloads,
            commands::servers::list_servers,
            commands::servers::edit_server_list,
            commands::server::get_server_config,
            commands::server::play_on_cinepol,
        ])
        .run(tauri::generate_context!())
}
