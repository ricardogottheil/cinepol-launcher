use launcher_core::application::list_game_versions::list_game_versions as list;
use tauri::State;

use crate::dto::{GameVersionDto, GameVersionsDto};
use crate::error::AppError;
use crate::state::AppState;

#[tauri::command]
pub async fn list_game_versions(
    state: State<'_, AppState>,
    include_snapshots: bool,
) -> Result<GameVersionsDto, AppError> {
    let (manifest, versions) = list(&state.versions, include_snapshots).await?;
    Ok(GameVersionsDto {
        latest_release: manifest.latest_release.as_str().to_owned(),
        versions: versions.iter().map(GameVersionDto::from).collect(),
    })
}
