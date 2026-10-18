use launcher_core::application::list_game_versions::list_game_versions as list;
use launcher_core::application::list_loader_versions::list_loader_versions as list_releases;
use launcher_core::domain::loader::LoaderKind;
use tauri::State;

use crate::dto::{GameVersionDto, GameVersionsDto, LoaderReleaseDto};
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

/// A loader's releases for a game version, newest first.
#[tauri::command]
pub async fn list_loader_versions(
    state: State<'_, AppState>,
    loader: String,
    game_version: String,
) -> Result<Vec<LoaderReleaseDto>, AppError> {
    let releases = match LoaderKind::from_id(&loader) {
        Some(LoaderKind::Fabric) => list_releases(&game_version, &state.fabric_releases).await?,
        Some(LoaderKind::Quilt) => list_releases(&game_version, &state.quilt_releases).await?,
        Some(LoaderKind::Forge) => list_releases(&game_version, &state.forge_releases).await?,
        Some(LoaderKind::NeoForge) => {
            list_releases(&game_version, &state.neoforge_releases).await?
        }
        _ => {
            return Err(AppError::invalid_input(format!(
                "{loader:?} has no release list"
            )));
        }
    };
    Ok(releases.iter().map(LoaderReleaseDto::from).collect())
}
