//! The servers the player saves in the launcher next to CINEPOL.

use launcher_core::application::list_saved_servers::list_saved_servers as list;
use launcher_core::application::ping_server::ping_server as ping;
use launcher_core::application::remove_saved_server::remove_saved_server as remove;
use launcher_core::application::save_server::{ServerForm, save_server as save};
use launcher_core::domain::saved_server::SavedServerId;
use tauri::State;

use crate::dto::{SavedServerDto, SavedServerFormDto, ServerStatusDto};
use crate::error::{AppError, ErrorCode};
use crate::state::AppState;

fn parse_saved_server_id(raw: &str) -> Result<SavedServerId, AppError> {
    SavedServerId::parse(raw)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "not a saved server id"))
}

#[tauri::command]
pub async fn list_saved_servers(
    state: State<'_, AppState>,
) -> Result<Vec<SavedServerDto>, AppError> {
    let servers = list(&state.saved_servers).await?;
    Ok(servers.iter().map(SavedServerDto::from).collect())
}

/// Adds a server, or changes the one with `form.id`.
#[tauri::command]
pub async fn save_server(
    state: State<'_, AppState>,
    form: SavedServerFormDto,
) -> Result<SavedServerDto, AppError> {
    let id = form.id.as_deref().map(parse_saved_server_id).transpose()?;
    let server = save(
        ServerForm {
            id,
            name: &form.name,
            address: &form.address,
            instance: form.instance_id.as_deref(),
        },
        &state.saved_servers,
        &state.instances,
    )
    .await?;
    Ok(SavedServerDto::from(&server))
}

#[tauri::command]
pub async fn remove_saved_server(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<(), AppError> {
    let id = parse_saved_server_id(&server_id)?;
    Ok(remove(id, &state.saved_servers).await?)
}

/// Pings any server address (Server List Ping).
#[tauri::command]
pub async fn ping_server(
    state: State<'_, AppState>,
    address: String,
) -> Result<ServerStatusDto, AppError> {
    let status = ping(&address, &state.pinger).await?;
    Ok(ServerStatusDto::from(&status))
}
