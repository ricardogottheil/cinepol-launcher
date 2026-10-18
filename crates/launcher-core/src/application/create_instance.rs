//! Creates a new instance from a display name, a game version and a loader,
//! and pre-seeds its server list.

use thiserror::Error;

use crate::domain::instance::{
    GameVersion, GameVersionError, Instance, InstanceId, InstanceName, InstanceNameError,
    InstanceOrigin, InstanceRepository, InstanceRepositoryError,
};
use crate::domain::loader::LoaderSpec;
use crate::domain::server_list::{ServerEntry, ServerListStore};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CreateInstanceError {
    #[error(transparent)]
    InvalidName(#[from] InstanceNameError),
    #[error(transparent)]
    InvalidVersion(#[from] GameVersionError),
    #[error(transparent)]
    Storage(#[from] InstanceRepositoryError),
}

pub struct NewInstance<'a> {
    pub name: &'a str,
    pub game_version: &'a str,
    pub loader: LoaderSpec,
    pub origin: InstanceOrigin,
    /// Put first in the new instance's `servers.dat` (the CINEPOL server).
    pub seed_server: Option<ServerEntry>,
}

/// The id is a path-safe slug of the name, made unique among existing
/// instances. Seeding happens once, at creation: a player who later removes
/// the entry does not get it back.
pub async fn create_instance<R: InstanceRepository, S: ServerListStore>(
    request: NewInstance<'_>,
    instances: &R,
    servers: &S,
) -> Result<Instance, CreateInstanceError> {
    let name = InstanceName::parse(request.name)?;
    let game_version = GameVersion::parse(request.game_version.trim())?;
    let mut taken: Vec<InstanceId> = instances
        .list()
        .await?
        .into_iter()
        .map(|instance| instance.id)
        .collect();
    let base = InstanceId::slug_from(&name);
    // `list` skips folders it cannot read; those ids are taken too.
    let id = loop {
        let candidate = InstanceId::unique(&base, |candidate| taken.contains(candidate));
        if !instances.id_taken(&candidate).await? {
            break candidate;
        }
        taken.push(candidate);
    };
    let instance = Instance::new(id, name, game_version, request.loader, request.origin);
    instances.save(&instance).await?;

    if let Some(entry) = request.seed_server {
        // The instance is usable without the entry, so a failed seed is not fatal.
        let seeded = async {
            let mut list = servers.load(&instance.id).await?;
            if list.seed_first(entry) {
                servers.save(&instance.id, &list).await?;
            }
            Ok::<(), crate::domain::server_list::ServerListStoreError>(())
        };
        if let Err(error) = seeded.await {
            tracing::warn!(instance = %instance.id, %error, "could not pre-seed the server list");
        }
    }
    Ok(instance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::server_list::{ServerAddress, ServerListStoreError};
    use crate::testing::{InMemoryInstanceRepository, InMemoryServerListStore};

    fn vanilla(name: &'static str, version: &'static str) -> NewInstance<'static> {
        NewInstance {
            name,
            game_version: version,
            loader: LoaderSpec::vanilla(),
            origin: InstanceOrigin::Custom,
            seed_server: None,
        }
    }

    fn cinepol_entry() -> ServerEntry {
        ServerEntry::new(
            "CINEPOL",
            ServerAddress::parse("mc.policarpiers.com").unwrap(),
        )
    }

    #[tokio::test]
    async fn creates_and_saves_with_a_unique_slug() {
        let repo = InMemoryInstanceRepository::default();
        let servers = InMemoryServerListStore::default();
        let first = create_instance(vanilla("Survival World", "26.2"), &repo, &servers)
            .await
            .unwrap();
        let second = create_instance(vanilla("Survival World", "1.21.11"), &repo, &servers)
            .await
            .unwrap();
        assert_eq!(first.id.as_str(), "survival-world");
        assert_eq!(second.id.as_str(), "survival-world-2");
        assert_eq!(repo.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn rejects_invalid_names_and_versions_without_saving() {
        let repo = InMemoryInstanceRepository::default();
        let servers = InMemoryServerListStore::default();
        assert!(matches!(
            create_instance(vanilla(" ", "26.2"), &repo, &servers).await,
            Err(CreateInstanceError::InvalidName(_))
        ));
        assert!(matches!(
            create_instance(vanilla("Ok", "../26.2"), &repo, &servers).await,
            Err(CreateInstanceError::InvalidVersion(_))
        ));
        assert!(repo.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn pre_seeds_the_server_list_of_the_new_instance() {
        let repo = InMemoryInstanceRepository::default();
        let servers = InMemoryServerListStore::default();
        let request = NewInstance {
            seed_server: Some(cinepol_entry()),
            ..vanilla("Survival World", "26.2")
        };
        let instance = create_instance(request, &repo, &servers).await.unwrap();
        let list = servers.load(&instance.id).await.unwrap();
        assert_eq!(list.entries(), [cinepol_entry()]);
    }

    #[tokio::test]
    async fn without_a_seed_the_server_list_stays_untouched() {
        let repo = InMemoryInstanceRepository::default();
        let servers = InMemoryServerListStore::default();
        let instance = create_instance(vanilla("Plain", "26.2"), &repo, &servers)
            .await
            .unwrap();
        assert!(
            servers
                .load(&instance.id)
                .await
                .unwrap()
                .entries()
                .is_empty()
        );
    }

    /// A `servers.dat` that cannot be read; remembers whether anything was saved.
    #[derive(Default)]
    struct CorruptServerList {
        saved: std::sync::Mutex<bool>,
    }

    impl ServerListStore for CorruptServerList {
        async fn load(
            &self,
            _instance: &InstanceId,
        ) -> Result<crate::domain::server_list::ServerList, ServerListStoreError> {
            Err(ServerListStoreError::Corrupt("garbage".into()))
        }

        async fn save(
            &self,
            _instance: &InstanceId,
            _list: &crate::domain::server_list::ServerList,
        ) -> Result<(), ServerListStoreError> {
            *self.saved.lock().unwrap() = true;
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_corrupt_server_list_is_never_overwritten_by_the_seed() {
        let repo = InMemoryInstanceRepository::default();
        let servers = CorruptServerList::default();
        let request = NewInstance {
            seed_server: Some(cinepol_entry()),
            ..vanilla("Survival World", "26.2")
        };
        let instance = create_instance(request, &repo, &servers).await.unwrap();
        assert_eq!(
            repo.list().await.unwrap(),
            [instance],
            "the instance is still created"
        );
        assert!(
            !*servers.saved.lock().unwrap(),
            "the unreadable file is left alone"
        );
    }
}
