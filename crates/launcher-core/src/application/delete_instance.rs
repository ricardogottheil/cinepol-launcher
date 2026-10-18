//! Deletes an instance: its metadata and its whole folder, worlds included.
//!
//! The caller makes sure the instance is not running, installing or updating
//! (the shell holds it like a running game while this runs). Deleting the
//! CINEPOL instance is allowed: Play on CINEPOL creates it again.

use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};

pub async fn delete_instance<R: InstanceRepository>(
    id: &InstanceId,
    instances: &R,
) -> Result<(), InstanceRepositoryError> {
    instances.delete(id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::instance::{GameVersion, Instance, InstanceName, InstanceOrigin};
    use crate::domain::loader::LoaderSpec;
    use crate::testing::InMemoryInstanceRepository;

    fn instance(id: &str) -> Instance {
        Instance::new(
            InstanceId::parse(id).unwrap(),
            InstanceName::parse(id).unwrap(),
            GameVersion::parse("26.2").unwrap(),
            LoaderSpec::vanilla(),
            InstanceOrigin::Custom,
        )
    }

    #[tokio::test]
    async fn deletes_only_the_given_instance() {
        let instances = InMemoryInstanceRepository::default();
        instances.save(&instance("survival")).await.unwrap();
        instances.save(&instance("creative")).await.unwrap();

        delete_instance(&InstanceId::parse("survival").unwrap(), &instances)
            .await
            .unwrap();

        let left: Vec<String> = instances
            .list()
            .await
            .unwrap()
            .iter()
            .map(|i| i.id.to_string())
            .collect();
        assert_eq!(left, ["creative"]);
    }

    #[tokio::test]
    async fn reports_an_unknown_instance() {
        let instances = InMemoryInstanceRepository::default();
        let id = InstanceId::parse("missing").unwrap();
        assert_eq!(
            delete_instance(&id, &instances).await,
            Err(InstanceRepositoryError::NotFound(id))
        );
    }
}
