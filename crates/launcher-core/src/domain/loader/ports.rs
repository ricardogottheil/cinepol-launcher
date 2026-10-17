use std::future::Future;

use thiserror::Error;

use super::{LoaderKind, LoaderRelease};
use crate::domain::instance::GameVersion;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LoaderMetadataError {
    #[error("{0:?} does not support Minecraft {1}")]
    UnsupportedGameVersion(LoaderKind, String),
    #[error("loader metadata request failed: {0}")]
    Network(String),
}

/// Lists the releases a loader offers for a game version (Fabric/Quilt meta,
/// Forge/NeoForge maven metadata). Installing is a separate port (Milestones 8-9).
pub trait LoaderMetadata: Send + Sync {
    fn kind(&self) -> LoaderKind;
    fn releases(
        &self,
        game_version: &GameVersion,
    ) -> impl Future<Output = Result<Vec<LoaderRelease>, LoaderMetadataError>> + Send;
}
