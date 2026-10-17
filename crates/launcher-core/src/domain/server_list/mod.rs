//! The multiplayer server list of an instance (`servers.dat`).

mod address;
mod ports;

pub use address::{DEFAULT_PORT, ServerAddress, ServerAddressError};
pub use ports::{ServerListStore, ServerListStoreError};

use thiserror::Error;

/// A field the launcher does not understand, kept byte-for-byte so saving the
/// list never drops data written by the game or by mods. Opaque to the domain:
/// only the storage adapter interprets `kind` and `payload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreservedField {
    pub name: String,
    pub kind: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preserved(pub Vec<PreservedField>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    pub name: String,
    pub address: ServerAddress,
    /// Base64 PNG the game caches for the server list.
    pub icon: Option<String>,
    /// `None` asks the player; `Some` remembers their resource-pack choice.
    pub accept_textures: Option<bool>,
    pub hidden: bool,
    pub preserved: Preserved,
}

impl ServerEntry {
    pub fn new(name: impl Into<String>, address: ServerAddress) -> Self {
        Self {
            name: name.into(),
            address,
            icon: None,
            accept_textures: None,
            hidden: false,
            preserved: Preserved::default(),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServerListError {
    #[error("no server at position {0}")]
    OutOfRange(usize),
}

/// Ordered servers plus any root fields the launcher does not understand.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerList {
    entries: Vec<ServerEntry>,
    pub preserved: Preserved,
}

impl ServerList {
    pub fn new(entries: Vec<ServerEntry>) -> Self {
        Self {
            entries,
            preserved: Preserved::default(),
        }
    }

    pub fn entries(&self) -> &[ServerEntry] {
        &self.entries
    }

    pub fn add(&mut self, entry: ServerEntry) {
        self.entries.push(entry);
    }

    pub fn replace(&mut self, index: usize, entry: ServerEntry) -> Result<(), ServerListError> {
        let slot = self
            .entries
            .get_mut(index)
            .ok_or(ServerListError::OutOfRange(index))?;
        *slot = entry;
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Result<ServerEntry, ServerListError> {
        if index >= self.entries.len() {
            return Err(ServerListError::OutOfRange(index));
        }
        Ok(self.entries.remove(index))
    }

    /// Moves the entry at `from` so it ends up at `to`.
    pub fn move_entry(&mut self, from: usize, to: usize) -> Result<(), ServerListError> {
        let len = self.entries.len();
        if from >= len {
            return Err(ServerListError::OutOfRange(from));
        }
        if to >= len {
            return Err(ServerListError::OutOfRange(to));
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        Ok(())
    }

    /// Puts `entry` first unless the list already reaches that server
    /// (the CINEPOL pre-seed). Returns whether it was inserted.
    pub fn seed_first(&mut self, entry: ServerEntry) -> bool {
        if self
            .entries
            .iter()
            .any(|existing| existing.address.same_server(&entry.address))
        {
            return false;
        }
        self.entries.insert(0, entry);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, address: &str) -> ServerEntry {
        ServerEntry::new(name, ServerAddress::parse(address).unwrap())
    }

    fn names(list: &ServerList) -> Vec<&str> {
        list.entries().iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn seed_first_inserts_at_the_top_once() {
        let mut list = ServerList::new(vec![entry("Hypixel", "mc.hypixel.net")]);
        assert!(list.seed_first(entry("CINEPOL", "mc.policarpiers.com")));
        assert!(!list.seed_first(entry("CINEPOL again", "MC.POLICARPIERS.COM:25565")));
        assert_eq!(names(&list), ["CINEPOL", "Hypixel"]);
    }

    #[test]
    fn seed_first_respects_a_removed_entry_that_the_player_re_added_elsewhere() {
        let mut list = ServerList::new(vec![
            entry("Mine", "mc.hypixel.net"),
            entry("My CINEPOL", "mc.policarpiers.com"),
        ]);
        assert!(!list.seed_first(entry("CINEPOL", "mc.policarpiers.com")));
        assert_eq!(names(&list), ["Mine", "My CINEPOL"]);
    }

    #[test]
    fn edits_reorders_and_removes_with_bounds_checks() {
        let mut list = ServerList::new(vec![
            entry("A", "a.example"),
            entry("B", "b.example"),
            entry("C", "c.example"),
        ]);
        list.move_entry(2, 0).unwrap();
        assert_eq!(names(&list), ["C", "A", "B"]);
        list.replace(1, entry("A2", "a2.example")).unwrap();
        assert_eq!(list.remove(0).unwrap().name, "C");
        assert_eq!(names(&list), ["A2", "B"]);
        assert_eq!(list.remove(5), Err(ServerListError::OutOfRange(5)));
        assert_eq!(list.move_entry(0, 9), Err(ServerListError::OutOfRange(9)));
        assert_eq!(
            list.replace(9, entry("X", "x.example")),
            Err(ServerListError::OutOfRange(9))
        );
    }

    #[test]
    fn edits_keep_preserved_fields_of_untouched_entries() {
        let mut kept = entry("Modded", "modded.example");
        kept.preserved = Preserved(vec![PreservedField {
            name: "forgeData".into(),
            kind: 10,
            payload: vec![1, 2, 3],
        }]);
        let mut list = ServerList::new(vec![entry("A", "a.example"), kept.clone()]);
        list.remove(0).unwrap();
        list.add(entry("B", "b.example"));
        assert_eq!(list.entries()[0], kept);
    }
}
