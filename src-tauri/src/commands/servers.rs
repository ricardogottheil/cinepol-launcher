//! An instance's multiplayer server list (`servers.dat`).

use launcher_core::application::edit_server_list::{
    ServerForm, ServerListEdit, edit_server_list as edit, list_servers as list,
};
use tauri::State;
use tokio_util::sync::CancellationToken;

use super::modpacks::hold;
use super::parse_instance_id;
use crate::dto::{ServerEditDto, ServerFormDto, ServerListDto};
use crate::error::AppError;
use crate::state::AppState;

#[tauri::command]
pub async fn list_servers(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<ServerListDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    Ok(ServerListDto::from(&list(&id, &state.servers).await?))
}

/// Applies one change. Refused while the game runs: it rewrites the list on exit.
#[tauri::command]
pub async fn edit_server_list(
    state: State<'_, AppState>,
    instance_id: String,
    change: ServerEditDto,
) -> Result<ServerListDto, AppError> {
    let id = parse_instance_id(&instance_id)?;
    let _claim = hold(&state, &id, &CancellationToken::new())?;
    let position = |index: u32| usize::try_from(index).unwrap_or(usize::MAX);
    let form = |form: ServerFormDto| ServerForm {
        name: form.name,
        address: form.address,
        accept_textures: form.accept_textures,
    };
    let change = match change {
        ServerEditDto::Add { form: input } => ServerListEdit::Add(form(input)),
        ServerEditDto::Update { index, form: input } => ServerListEdit::Update {
            index: position(index),
            form: form(input),
        },
        ServerEditDto::Remove { index } => ServerListEdit::Remove {
            index: position(index),
        },
        ServerEditDto::Move { from, to } => ServerListEdit::Move {
            from: position(from),
            to: position(to),
        },
    };
    Ok(ServerListDto::from(
        &edit(&id, change, &state.servers).await?,
    ))
}
