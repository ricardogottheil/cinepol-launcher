//! The servers the player saved, in the order they were added.

use crate::domain::saved_server::{SavedServer, SavedServerRepository, SavedServerRepositoryError};

pub async fn list_saved_servers<S: SavedServerRepository>(
    servers: &S,
) -> Result<Vec<SavedServer>, SavedServerRepositoryError> {
    servers.list().await
}
