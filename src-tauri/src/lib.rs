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
mod operations;
mod progress;
mod state;

/// Builds and runs the Tauri application.
pub fn run() -> Result<(), tauri::Error> {
    tauri::Builder::default()
        .manage(state::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::demo::run_progress_demo,
            commands::demo::cancel_operation,
        ])
        .run(tauri::generate_context!())
}
