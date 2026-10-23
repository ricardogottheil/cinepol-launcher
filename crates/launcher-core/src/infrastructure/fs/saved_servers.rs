//! `servers.json`: the servers the player added to the launcher.

use std::io::ErrorKind;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::write_atomic;
use crate::domain::instance::InstanceId;
use crate::domain::saved_server::{
    SavedServer, SavedServerId, SavedServerRepository, SavedServerRepositoryError, ServerName,
};
use crate::domain::server_list::ServerAddress;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ServersFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    servers: Vec<ServerRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerRecord {
    id: Uuid,
    name: String,
    address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance: Option<String>,
}

impl From<&SavedServer> for ServerRecord {
    fn from(server: &SavedServer) -> Self {
        Self {
            id: server.id.as_uuid(),
            name: server.name.as_str().to_owned(),
            address: server.address.to_string(),
            instance: server.instance.as_ref().map(|id| id.as_str().to_owned()),
        }
    }
}

impl ServerRecord {
    /// An entry edited into something invalid is dropped, never trusted.
    fn into_server(self) -> Option<SavedServer> {
        Some(SavedServer {
            id: SavedServerId::parse(&self.id.to_string())?,
            name: ServerName::parse(&self.name).ok()?,
            address: ServerAddress::parse(&self.address).ok()?,
            instance: self.instance.and_then(|raw| InstanceId::parse(&raw).ok()),
        })
    }
}

/// Saved servers in one JSON file. A lock serializes read-modify-write cycles.
#[derive(Debug)]
pub struct JsonSavedServerRepository {
    file: PathBuf,
    lock: Mutex<()>,
}

impl JsonSavedServerRepository {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            lock: Mutex::new(()),
        }
    }

    async fn load(&self) -> Result<ServersFile, SavedServerRepositoryError> {
        match tokio::fs::read(&self.file).await {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(data) => Ok(data),
                Err(error) => self.set_aside_corrupt_file(&error.to_string()).await,
            },
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(ServersFile::default()),
            Err(e) => Err(SavedServerRepositoryError::Storage(e.to_string())),
        }
    }

    /// A corrupt file would hide every saved server forever. Keep it next to
    /// the original for recovery and start over with an empty list.
    async fn set_aside_corrupt_file(
        &self,
        reason: &str,
    ) -> Result<ServersFile, SavedServerRepositoryError> {
        let stamp = Uuid::new_v4().simple();
        let backup = self.file.with_file_name(format!(
            "{}.corrupt-{stamp}",
            self.file
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default()
        ));
        tokio::fs::rename(&self.file, &backup).await.map_err(|e| {
            SavedServerRepositoryError::Storage(format!("{reason}; could not set it aside: {e}"))
        })?;
        tracing::warn!(backup = %backup.display(), %reason, "servers file was unreadable and was set aside");
        Ok(ServersFile::default())
    }

    async fn store(&self, mut data: ServersFile) -> Result<(), SavedServerRepositoryError> {
        data.schema_version = SCHEMA_VERSION;
        let bytes = serde_json::to_vec_pretty(&data)
            .map_err(|e| SavedServerRepositoryError::Storage(e.to_string()))?;
        write_atomic(&self.file, &bytes)
            .await
            .map_err(|e| SavedServerRepositoryError::Storage(e.to_string()))
    }
}

impl SavedServerRepository for JsonSavedServerRepository {
    async fn list(&self) -> Result<Vec<SavedServer>, SavedServerRepositoryError> {
        let _guard = self.lock.lock().await;
        Ok(self
            .load()
            .await?
            .servers
            .into_iter()
            .filter_map(ServerRecord::into_server)
            .collect())
    }

    async fn save(&self, server: &SavedServer) -> Result<(), SavedServerRepositoryError> {
        let _guard = self.lock.lock().await;
        let mut data = self.load().await?;
        let record = ServerRecord::from(server);
        match data
            .servers
            .iter_mut()
            .find(|existing| existing.id == record.id)
        {
            Some(existing) => *existing = record,
            None => data.servers.push(record),
        }
        self.store(data).await
    }

    async fn remove(&self, id: SavedServerId) -> Result<bool, SavedServerRepositoryError> {
        let _guard = self.lock.lock().await;
        let mut data = self.load().await?;
        let before = data.servers.len();
        data.servers.retain(|record| record.id != id.as_uuid());
        let removed = data.servers.len() != before;
        if removed {
            self.store(data).await?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(name: &str, address: &str, instance: Option<&str>) -> SavedServer {
        SavedServer {
            id: SavedServerId::new_random(),
            name: ServerName::parse(name).unwrap(),
            address: ServerAddress::parse(address).unwrap(),
            instance: instance.map(|id| InstanceId::parse(id).unwrap()),
        }
    }

    #[tokio::test]
    async fn keeps_servers_in_order_and_replaces_one_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonSavedServerRepository::new(dir.path().join("servers.json"));
        assert!(repo.list().await.unwrap().is_empty());

        let mut hypixel = server("Hypixel", "mc.hypixel.net", None);
        let local = server("Local", "127.0.0.1:25566", Some("vanilla-26-2"));
        repo.save(&hypixel).await.unwrap();
        repo.save(&local).await.unwrap();
        hypixel.instance = Some(InstanceId::parse("skyblock").unwrap());
        repo.save(&hypixel).await.unwrap();

        assert_eq!(repo.list().await.unwrap(), [hypixel.clone(), local.clone()]);
        assert!(repo.remove(local.id).await.unwrap());
        assert!(!repo.remove(local.id).await.unwrap());
        assert_eq!(repo.list().await.unwrap(), [hypixel]);
    }

    #[tokio::test]
    async fn drops_entries_edited_into_something_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("servers.json");
        let good = Uuid::new_v4();
        let body = format!(
            r#"{{"schemaVersion":1,"servers":[
                {{"id":"{good}","name":"Good","address":"play.example.net","instance":"../escape"}},
                {{"id":"{}","name":"Bad host","address":"bad host!"}},
                {{"id":"{}","name":"","address":"empty.example.net"}}
            ]}}"#,
            Uuid::new_v4(),
            Uuid::new_v4()
        );
        tokio::fs::write(&file, body).await.unwrap();
        let servers = JsonSavedServerRepository::new(&file).list().await.unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name.as_str(), "Good");
        assert_eq!(servers[0].instance, None);
    }

    #[tokio::test]
    async fn sets_a_corrupt_file_aside_and_starts_over() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("servers.json");
        tokio::fs::write(&file, b"{not json").await.unwrap();
        let repo = JsonSavedServerRepository::new(&file);
        assert!(repo.list().await.unwrap().is_empty());
        let kept: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            kept.iter()
                .any(|name| name.starts_with("servers.json.corrupt-")),
            "{kept:?}"
        );
    }
}
