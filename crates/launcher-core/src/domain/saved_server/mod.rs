//! Servers the player keeps in the launcher next to CINEPOL, each with the
//! instance it is played with. Not the game's own `servers.dat` list.

use std::future::Future;

use thiserror::Error;
use uuid::Uuid;

use crate::domain::instance::InstanceId;
use crate::domain::server_list::ServerAddress;

const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SavedServerId(Uuid);

impl SavedServerId {
    pub fn new_random() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Uuid::try_parse(raw).ok().map(Self)
    }

    pub fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ServerNameError {
    #[error("a server needs a name")]
    Empty,
    #[error("a server name is at most {MAX_NAME_CHARS} characters")]
    TooLong,
    #[error("a server name cannot hold control characters")]
    ControlCharacters,
}

/// What the player calls a server, trimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerName(String);

impl ServerName {
    pub fn parse(raw: &str) -> Result<Self, ServerNameError> {
        let name = raw.trim();
        if name.is_empty() {
            return Err(ServerNameError::Empty);
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(ServerNameError::TooLong);
        }
        if name.chars().any(char::is_control) {
            return Err(ServerNameError::ControlCharacters);
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedServer {
    pub id: SavedServerId,
    pub name: ServerName,
    pub address: ServerAddress,
    /// The instance Play uses; `None` until the player picks one.
    pub instance: Option<InstanceId>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SavedServerRepositoryError {
    #[error("could not read or write the saved servers: {0}")]
    Storage(String),
}

/// The saved servers, in the order they were added.
pub trait SavedServerRepository: Send + Sync {
    fn list(
        &self,
    ) -> impl Future<Output = Result<Vec<SavedServer>, SavedServerRepositoryError>> + Send;
    /// Adds the server, or replaces the one with the same id in place.
    fn save(
        &self,
        server: &SavedServer,
    ) -> impl Future<Output = Result<(), SavedServerRepositoryError>> + Send;
    /// `false` when no server had that id.
    fn remove(
        &self,
        id: SavedServerId,
    ) -> impl Future<Output = Result<bool, SavedServerRepositoryError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(
            ServerName::parse("  Hypixel  ").unwrap().as_str(),
            "Hypixel"
        );
        assert_eq!(
            ServerName::parse(&"\u{e1}".repeat(64)).unwrap().as_str(),
            "\u{e1}".repeat(64)
        );
        assert_eq!(ServerName::parse("   "), Err(ServerNameError::Empty));
        assert_eq!(
            ServerName::parse(&"a".repeat(65)),
            Err(ServerNameError::TooLong)
        );
        assert_eq!(
            ServerName::parse("bad\nname"),
            Err(ServerNameError::ControlCharacters)
        );
    }

    #[test]
    fn ids_round_trip_and_refuse_garbage() {
        let id = SavedServerId::new_random();
        assert_eq!(SavedServerId::parse(&id.as_uuid().to_string()), Some(id));
        assert_eq!(SavedServerId::parse("../servers"), None);
    }
}
