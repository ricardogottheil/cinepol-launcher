//! Lists the player's instances, sorted by name.

use crate::domain::instance::{Instance, InstanceRepository, InstanceRepositoryError};

pub async fn list_instances<R: InstanceRepository>(
    instances: &R,
) -> Result<Vec<Instance>, InstanceRepositoryError> {
    instances.list().await
}
