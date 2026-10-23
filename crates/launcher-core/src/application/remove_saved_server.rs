//! Forget a saved server. Its instance stays.

use thiserror::Error;

use crate::domain::saved_server::{
    SavedServerId, SavedServerRepository, SavedServerRepositoryError,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RemoveSavedServerError {
    #[error("no saved server with that id")]
    NotFound,
    #[error(transparent)]
    Storage(#[from] SavedServerRepositoryError),
}

pub async fn remove_saved_server<S: SavedServerRepository>(
    id: SavedServerId,
    servers: &S,
) -> Result<(), RemoveSavedServerError> {
    if servers.remove(id).await? {
        Ok(())
    } else {
        Err(RemoveSavedServerError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::saved_server::{SavedServer, ServerName};
    use crate::domain::server_list::ServerAddress;
    use crate::testing::InMemorySavedServerRepository;

    #[tokio::test]
    async fn removes_a_saved_server_and_reports_unknown_ones() {
        let server = SavedServer {
            id: SavedServerId::new_random(),
            name: ServerName::parse("Hypixel").unwrap(),
            address: ServerAddress::parse("mc.hypixel.net").unwrap(),
            instance: None,
        };
        let servers = InMemorySavedServerRepository::with(vec![server.clone()]);
        remove_saved_server(server.id, &servers).await.unwrap();
        assert!(servers.list().await.unwrap().is_empty());
        assert_eq!(
            remove_saved_server(server.id, &servers).await,
            Err(RemoveSavedServerError::NotFound)
        );
    }
}
