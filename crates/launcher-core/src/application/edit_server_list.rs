//! Lists and edits an instance's multiplayer servers (`servers.dat`).
//!
//! Editing a server changes only what the player sees in the form (name,
//! address, resource-pack choice); its cached icon, hidden flag and any tags
//! the launcher does not understand are kept, so saving never drops data the
//! game or mods wrote. The caller makes sure the game is not running, because
//! the game rewrites `servers.dat` when it exits.

use thiserror::Error;

use crate::domain::instance::InstanceId;
use crate::domain::server_list::{
    ServerAddress, ServerAddressError, ServerEntry, ServerList, ServerListError, ServerListStore,
    ServerListStoreError,
};

/// Server names are at most this many characters.
pub const MAX_SERVER_NAME_CHARS: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EditServerListError {
    #[error(
        "the server name must be 1 to {MAX_SERVER_NAME_CHARS} characters without control characters"
    )]
    InvalidName,
    #[error(transparent)]
    InvalidAddress(#[from] ServerAddressError),
    #[error(transparent)]
    OutOfRange(#[from] ServerListError),
    #[error(transparent)]
    Storage(#[from] ServerListStoreError),
}

/// What the player typed for a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerForm {
    pub name: String,
    pub address: String,
    /// `None` asks in game; `Some` remembers the resource-pack choice.
    pub accept_textures: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerListEdit {
    Add(ServerForm),
    Update {
        index: usize,
        form: ServerForm,
    },
    Remove {
        index: usize,
    },
    /// Moves the server at `from` so it ends up at `to`.
    Move {
        from: usize,
        to: usize,
    },
}

pub async fn list_servers<S: ServerListStore>(
    instance: &InstanceId,
    servers: &S,
) -> Result<ServerList, EditServerListError> {
    Ok(servers.load(instance).await?)
}

/// Applies one edit and saves the list; nothing is saved when it is invalid.
pub async fn edit_server_list<S: ServerListStore>(
    instance: &InstanceId,
    edit: ServerListEdit,
    servers: &S,
) -> Result<ServerList, EditServerListError> {
    let mut list = servers.load(instance).await?;
    match edit {
        ServerListEdit::Add(form) => {
            let (name, address) = validate(&form)?;
            let mut entry = ServerEntry::new(name, address);
            entry.accept_textures = form.accept_textures;
            list.add(entry);
        }
        ServerListEdit::Update { index, form } => {
            let (name, address) = validate(&form)?;
            let mut entry = list
                .entries()
                .get(index)
                .cloned()
                .ok_or(ServerListError::OutOfRange(index))?;
            entry.name = name;
            entry.address = address;
            entry.accept_textures = form.accept_textures;
            list.replace(index, entry)?;
        }
        ServerListEdit::Remove { index } => {
            list.remove(index)?;
        }
        ServerListEdit::Move { from, to } => list.move_entry(from, to)?,
    }
    servers.save(instance, &list).await?;
    Ok(list)
}

fn validate(form: &ServerForm) -> Result<(String, ServerAddress), EditServerListError> {
    let name = form.name.trim();
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_SERVER_NAME_CHARS
        && !name.chars().any(char::is_control);
    if !valid {
        return Err(EditServerListError::InvalidName);
    }
    Ok((name.to_owned(), ServerAddress::parse(form.address.trim())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::server_list::{Preserved, PreservedField};
    use crate::testing::InMemoryServerListStore;

    fn id() -> InstanceId {
        InstanceId::parse("survival").unwrap()
    }

    fn form(name: &str, address: &str) -> ServerForm {
        ServerForm {
            name: name.into(),
            address: address.into(),
            accept_textures: None,
        }
    }

    async fn store_with(entries: &[(&str, &str)]) -> InMemoryServerListStore {
        let store = InMemoryServerListStore::default();
        let mut list = ServerList::new(
            entries
                .iter()
                .map(|(name, address)| {
                    ServerEntry::new(*name, ServerAddress::parse(address).unwrap())
                })
                .collect(),
        );
        list.preserved = Preserved(vec![PreservedField {
            name: "root".into(),
            kind: 1,
            payload: vec![7],
        }]);
        store.save(&id(), &list).await.unwrap();
        store
    }

    fn names(list: &ServerList) -> Vec<&str> {
        list.entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    #[tokio::test]
    async fn lists_the_saved_servers() {
        let store = store_with(&[("CINEPOL", "mc.policarpiers.com")]).await;
        let list = list_servers(&id(), &store).await.unwrap();
        assert_eq!(names(&list), ["CINEPOL"]);
    }

    #[tokio::test]
    async fn adds_edits_moves_and_removes_servers() {
        let store = store_with(&[("CINEPOL", "mc.policarpiers.com")]).await;
        let instance = id();
        let edit = |edit| edit_server_list(&instance, edit, &store);

        let list = edit(ServerListEdit::Add(ServerForm {
            accept_textures: Some(true),
            ..form("  Hypixel  ", "mc.hypixel.net:25565")
        }))
        .await
        .unwrap();
        assert_eq!(names(&list), ["CINEPOL", "Hypixel"]);
        assert_eq!(list.entries()[1].address.host(), "mc.hypixel.net");
        assert_eq!(list.entries()[1].accept_textures, Some(true));

        let list = edit(ServerListEdit::Move { from: 1, to: 0 }).await.unwrap();
        assert_eq!(names(&list), ["Hypixel", "CINEPOL"]);

        let list = edit(ServerListEdit::Remove { index: 0 }).await.unwrap();
        assert_eq!(names(&list), ["CINEPOL"]);

        // Every edit is saved, and root fields the launcher does not know survive.
        let stored = store.load(&id()).await.unwrap();
        assert_eq!(stored, list);
        assert_eq!(stored.preserved.0.len(), 1);
    }

    #[tokio::test]
    async fn an_update_keeps_the_icon_hidden_flag_and_unknown_tags() {
        let store = InMemoryServerListStore::default();
        let mut entry = ServerEntry::new("Old", ServerAddress::parse("old.example.com").unwrap());
        entry.icon = Some("iVBORw0KGgo=".into());
        entry.hidden = true;
        entry.preserved = Preserved(vec![PreservedField {
            name: "forgeData".into(),
            kind: 10,
            payload: vec![1, 2, 3],
        }]);
        store
            .save(&id(), &ServerList::new(vec![entry.clone()]))
            .await
            .unwrap();

        let list = edit_server_list(
            &id(),
            ServerListEdit::Update {
                index: 0,
                form: ServerForm {
                    accept_textures: Some(false),
                    ..form("New", "new.example.com:25570")
                },
            },
            &store,
        )
        .await
        .unwrap();

        let updated = &list.entries()[0];
        assert_eq!(updated.name, "New");
        assert_eq!(updated.address.effective_port(), 25570);
        assert_eq!(updated.accept_textures, Some(false));
        assert_eq!(updated.icon, entry.icon);
        assert!(updated.hidden);
        assert_eq!(updated.preserved, entry.preserved);
    }

    #[tokio::test]
    async fn invalid_edits_leave_the_list_as_it_was() {
        let store = store_with(&[("CINEPOL", "mc.policarpiers.com")]).await;
        let before = store.load(&id()).await.unwrap();
        let cases = [
            ServerListEdit::Add(form("  ", "mc.example.com")),
            ServerListEdit::Add(form(
                &"x".repeat(MAX_SERVER_NAME_CHARS + 1),
                "mc.example.com",
            )),
            ServerListEdit::Add(form("Bad\nname", "mc.example.com")),
            ServerListEdit::Add(form("Bad", "not a host:99999")),
            ServerListEdit::Update {
                index: 5,
                form: form("X", "mc.example.com"),
            },
            ServerListEdit::Remove { index: 3 },
            ServerListEdit::Move { from: 0, to: 9 },
        ];
        for edit in cases {
            let label = format!("{edit:?}");
            assert!(
                edit_server_list(&id(), edit, &store).await.is_err(),
                "{label}"
            );
            assert_eq!(store.load(&id()).await.unwrap(), before, "{label}");
        }
    }
}
