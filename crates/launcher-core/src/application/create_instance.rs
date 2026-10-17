//! Creates a new instance from a display name, a game version and a loader.

use thiserror::Error;

use crate::domain::instance::{
    GameVersion, GameVersionError, Instance, InstanceId, InstanceName, InstanceNameError,
    InstanceOrigin, InstanceRepository, InstanceRepositoryError,
};
use crate::domain::loader::LoaderSpec;

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
}

/// The id is a path-safe slug of the name, made unique among existing instances.
pub async fn create_instance<R: InstanceRepository>(
    request: NewInstance<'_>,
    instances: &R,
) -> Result<Instance, CreateInstanceError> {
    let name = InstanceName::parse(request.name)?;
    let game_version = GameVersion::parse(request.game_version.trim())?;
    let existing: Vec<InstanceId> = instances
        .list()
        .await?
        .into_iter()
        .map(|instance| instance.id)
        .collect();
    let id = InstanceId::unique(&InstanceId::slug_from(&name), |candidate| {
        existing.contains(candidate)
    });
    let instance = Instance::new(id, name, game_version, request.loader, request.origin);
    instances.save(&instance).await?;
    Ok(instance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::InMemoryInstanceRepository;

    fn vanilla(name: &'static str, version: &'static str) -> NewInstance<'static> {
        NewInstance {
            name,
            game_version: version,
            loader: LoaderSpec::vanilla(),
            origin: InstanceOrigin::Custom,
        }
    }

    #[tokio::test]
    async fn creates_and_saves_with_a_unique_slug() {
        let repo = InMemoryInstanceRepository::default();
        let first = create_instance(vanilla("Survival World", "26.2"), &repo)
            .await
            .unwrap();
        let second = create_instance(vanilla("Survival World", "1.21.11"), &repo)
            .await
            .unwrap();
        assert_eq!(first.id.as_str(), "survival-world");
        assert_eq!(second.id.as_str(), "survival-world-2");
        assert_eq!(repo.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn rejects_invalid_names_and_versions_without_saving() {
        let repo = InMemoryInstanceRepository::default();
        assert!(matches!(
            create_instance(vanilla(" ", "26.2"), &repo).await,
            Err(CreateInstanceError::InvalidName(_))
        ));
        assert!(matches!(
            create_instance(vanilla("Ok", "../26.2"), &repo).await,
            Err(CreateInstanceError::InvalidVersion(_))
        ));
        assert!(repo.list().await.unwrap().is_empty());
    }
}
