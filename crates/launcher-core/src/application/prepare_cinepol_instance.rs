//! Finds or creates the managed CINEPOL instance and keeps it on the server's
//! recommended version.

use crate::application::create_instance::{CreateInstanceError, NewInstance, create_instance};
use crate::domain::instance::{Instance, InstanceOrigin, InstanceRepository};
use crate::domain::loader::LoaderSpec;
use crate::domain::server::ServerConfig;
use crate::domain::server_list::{ServerEntry, ServerListStore};

/// The server entry new instances get first in their `servers.dat`.
pub fn cinepol_server_entry(config: &ServerConfig) -> ServerEntry {
    ServerEntry::new(config.name.clone(), config.address.clone())
}

/// Reuses the instance whose origin is `CinepolServer` (moving it to the
/// recommended version when the server changed it), or creates one named
/// after the server and seeded with its address.
pub async fn prepare_cinepol_instance<R: InstanceRepository, S: ServerListStore>(
    config: &ServerConfig,
    instances: &R,
    servers: &S,
) -> Result<Instance, CreateInstanceError> {
    let recommended = config.versions.recommended();
    let existing = instances
        .list()
        .await?
        .into_iter()
        .find(|instance| instance.origin == InstanceOrigin::CinepolServer);

    match existing {
        Some(mut instance) => {
            if &instance.game_version != recommended {
                tracing::info!(from = %instance.game_version.as_str(), to = %recommended.as_str(), "moving the CINEPOL instance to the recommended version");
                instance.game_version = recommended.clone();
                instances.save(&instance).await?;
            }
            Ok(instance)
        }
        None => {
            let request = NewInstance {
                name: &config.name,
                game_version: recommended.as_str(),
                loader: LoaderSpec::vanilla(),
                origin: InstanceOrigin::CinepolServer,
                seed_server: Some(cinepol_server_entry(config)),
            };
            create_instance(request, instances, servers).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::load_server_config::fixtures::cinepol;
    use crate::domain::instance::{GameVersion, InstanceId, InstanceName};
    use crate::testing::{InMemoryInstanceRepository, InMemoryServerListStore};

    #[tokio::test]
    async fn creates_a_seeded_instance_on_the_recommended_version_once() {
        let instances = InMemoryInstanceRepository::default();
        let servers = InMemoryServerListStore::default();
        let config = cinepol("26.2");

        let first = prepare_cinepol_instance(&config, &instances, &servers)
            .await
            .unwrap();
        assert_eq!(first.name.as_str(), "CINEPOL");
        assert_eq!(first.game_version.as_str(), "26.2");
        assert_eq!(first.origin, InstanceOrigin::CinepolServer);
        assert_eq!(
            servers.load(&first.id).await.unwrap().entries(),
            [cinepol_server_entry(&config)]
        );

        let again = prepare_cinepol_instance(&config, &instances, &servers)
            .await
            .unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(instances.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn follows_the_recommended_version_without_reseeding() {
        let instances = InMemoryInstanceRepository::default();
        let servers = InMemoryServerListStore::default();
        let created = prepare_cinepol_instance(&cinepol("26.2"), &instances, &servers)
            .await
            .unwrap();
        // The player removed the CINEPOL entry; it must not come back.
        servers
            .save(&created.id, &Default::default())
            .await
            .unwrap();

        let moved = prepare_cinepol_instance(&cinepol("26.3"), &instances, &servers)
            .await
            .unwrap();
        assert_eq!(moved.game_version.as_str(), "26.3");
        assert_eq!(
            instances
                .get(&created.id)
                .await
                .unwrap()
                .game_version
                .as_str(),
            "26.3"
        );
        assert!(
            servers
                .load(&created.id)
                .await
                .unwrap()
                .entries()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn ignores_custom_instances_on_the_same_version() {
        let instances = InMemoryInstanceRepository::default();
        let servers = InMemoryServerListStore::default();
        instances
            .save(&Instance::new(
                InstanceId::parse("my-world").unwrap(),
                InstanceName::parse("My world").unwrap(),
                GameVersion::parse("26.2").unwrap(),
                LoaderSpec::vanilla(),
                InstanceOrigin::Custom,
            ))
            .await
            .unwrap();
        let cinepol_instance = prepare_cinepol_instance(&cinepol("26.2"), &instances, &servers)
            .await
            .unwrap();
        assert_eq!(cinepol_instance.id.as_str(), "cinepol");
        assert_eq!(instances.list().await.unwrap().len(), 2);
    }
}
