//! Add a server to the launcher, or change one already saved.

use thiserror::Error;

use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::saved_server::{
    SavedServer, SavedServerId, SavedServerRepository, SavedServerRepositoryError, ServerName,
    ServerNameError,
};
use crate::domain::server_list::{ServerAddress, ServerAddressError};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SaveServerError {
    #[error(transparent)]
    InvalidName(#[from] ServerNameError),
    #[error(transparent)]
    InvalidAddress(#[from] ServerAddressError),
    #[error("no instance {0}")]
    UnknownInstance(String),
    #[error("no saved server with that id")]
    NotFound,
    #[error("that server is already saved as {0}")]
    AlreadySaved(String),
    #[error(transparent)]
    Storage(#[from] SavedServerRepositoryError),
    #[error(transparent)]
    Instances(InstanceRepositoryError),
}

/// What the player typed. `id` is `None` for a new server.
#[derive(Debug, Clone, Copy)]
pub struct ServerForm<'a> {
    pub id: Option<SavedServerId>,
    pub name: &'a str,
    pub address: &'a str,
    pub instance: Option<&'a str>,
}

/// Saves the server once its name, address and instance check out. The same
/// address (case and default port aside) is never saved twice.
pub async fn save_server<S, I>(
    form: ServerForm<'_>,
    servers: &S,
    instances: &I,
) -> Result<SavedServer, SaveServerError>
where
    S: SavedServerRepository,
    I: InstanceRepository,
{
    let name = ServerName::parse(form.name)?;
    let address = ServerAddress::parse(form.address)?;
    let instance = match form.instance {
        None => None,
        Some(raw) => {
            let id = InstanceId::parse(raw)
                .map_err(|_| SaveServerError::UnknownInstance(raw.to_owned()))?;
            match instances.get(&id).await {
                Ok(_) => Some(id),
                Err(InstanceRepositoryError::NotFound(_)) => {
                    return Err(SaveServerError::UnknownInstance(raw.to_owned()));
                }
                Err(other) => return Err(SaveServerError::Instances(other)),
            }
        }
    };
    let saved = servers.list().await?;
    if let Some(id) = form.id
        && !saved.iter().any(|server| server.id == id)
    {
        return Err(SaveServerError::NotFound);
    }
    if let Some(twin) = saved
        .iter()
        .find(|server| Some(server.id) != form.id && server.address.same_server(&address))
    {
        return Err(SaveServerError::AlreadySaved(twin.name.as_str().to_owned()));
    }
    let server = SavedServer {
        id: form.id.unwrap_or_else(SavedServerId::new_random),
        name,
        address,
        instance,
    };
    servers.save(&server).await?;
    Ok(server)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::instance::GameVersion;
    use crate::domain::instance::{Instance, InstanceName, InstanceOrigin};
    use crate::domain::loader::LoaderSpec;
    use crate::testing::{InMemoryInstanceRepository, InMemorySavedServerRepository};

    async fn instances(ids: &[&str]) -> InMemoryInstanceRepository {
        let repo = InMemoryInstanceRepository::default();
        for id in ids {
            repo.save(&Instance::new(
                InstanceId::parse(id).unwrap(),
                InstanceName::parse(id).unwrap(),
                GameVersion::parse("26.2").unwrap(),
                LoaderSpec::vanilla(),
                InstanceOrigin::Custom,
            ))
            .await
            .unwrap();
        }
        repo
    }

    fn form<'a>(name: &'a str, address: &'a str, instance: Option<&'a str>) -> ServerForm<'a> {
        ServerForm {
            id: None,
            name,
            address,
            instance,
        }
    }

    #[tokio::test]
    async fn saves_a_new_server_and_edits_it_in_place() {
        let servers = InMemorySavedServerRepository::default();
        let instances = instances(&["skyblock"]).await;
        let saved = save_server(
            form(" Hypixel ", "mc.hypixel.net", None),
            &servers,
            &instances,
        )
        .await
        .unwrap();
        assert_eq!(saved.name.as_str(), "Hypixel");
        assert_eq!(saved.instance, None);

        let edited = save_server(
            ServerForm {
                id: Some(saved.id),
                ..form("Hypixel", "MC.Hypixel.net:25565", Some("skyblock"))
            },
            &servers,
            &instances,
        )
        .await
        .unwrap();
        assert_eq!(edited.id, saved.id);
        assert_eq!(
            edited.instance,
            Some(InstanceId::parse("skyblock").unwrap())
        );
        assert_eq!(servers.list().await.unwrap(), [edited]);
    }

    #[tokio::test]
    async fn never_saves_the_same_server_twice() {
        let servers = InMemorySavedServerRepository::default();
        let instances = instances(&[]).await;
        save_server(
            form("Hypixel", "mc.hypixel.net", None),
            &servers,
            &instances,
        )
        .await
        .unwrap();
        assert_eq!(
            save_server(
                form("Again", "MC.HYPIXEL.NET:25565", None),
                &servers,
                &instances
            )
            .await,
            Err(SaveServerError::AlreadySaved("Hypixel".into()))
        );
        assert_eq!(servers.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn refuses_bad_input_without_saving() {
        let servers = InMemorySavedServerRepository::default();
        let instances = instances(&[]).await;
        assert_eq!(
            save_server(form("", "mc.hypixel.net", None), &servers, &instances).await,
            Err(SaveServerError::InvalidName(ServerNameError::Empty))
        );
        assert!(matches!(
            save_server(form("Bad", "not a host", None), &servers, &instances).await,
            Err(SaveServerError::InvalidAddress(_))
        ));
        assert_eq!(
            save_server(
                form("Missing", "mc.hypixel.net", Some("gone")),
                &servers,
                &instances
            )
            .await,
            Err(SaveServerError::UnknownInstance("gone".into()))
        );
        assert_eq!(
            save_server(
                form("Escape", "mc.hypixel.net", Some("../x")),
                &servers,
                &instances
            )
            .await,
            Err(SaveServerError::UnknownInstance("../x".into()))
        );
        assert_eq!(
            save_server(
                ServerForm {
                    id: Some(SavedServerId::new_random()),
                    ..form("Ghost", "mc.hypixel.net", None)
                },
                &servers,
                &instances
            )
            .await,
            Err(SaveServerError::NotFound)
        );
        assert!(servers.list().await.unwrap().is_empty());
    }
}
