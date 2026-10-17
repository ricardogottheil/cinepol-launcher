//! Cinepol Launcher desktop shell.
//!
//! This crate is a driving adapter only: it builds the Tauri app, wires
//! infrastructure adapters into use cases (`state`), and exposes thin
//! commands (`commands`). Business logic lives in `cinepol-launcher-core`.

pub mod bindings;
mod commands;
mod dto;
mod events;
mod state;

/// Builds and runs the Tauri application.
pub fn run() -> Result<(), tauri::Error> {
    tauri::Builder::default()
        .manage(state::AppState::new())
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
}
