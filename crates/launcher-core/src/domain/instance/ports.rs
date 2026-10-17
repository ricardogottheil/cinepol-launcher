use std::future::Future;

use thiserror::Error;

use super::{Instance, InstanceId};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstanceRepositoryError {
    #[error("instance {0} does not exist")]
    NotFound(InstanceId),
    #[error("instance storage failed: {0}")]
    Storage(String),
}

/// Persists instance metadata (`instance.json` in each instance directory).
pub trait InstanceRepository: Send + Sync {
    fn list(&self) -> impl Future<Output = Result<Vec<Instance>, InstanceRepositoryError>> + Send;
    fn get(
        &self,
        id: &InstanceId,
    ) -> impl Future<Output = Result<Instance, InstanceRepositoryError>> + Send;
    fn save(
        &self,
        instance: &Instance,
    ) -> impl Future<Output = Result<(), InstanceRepositoryError>> + Send;
    fn delete(
        &self,
        id: &InstanceId,
    ) -> impl Future<Output = Result<(), InstanceRepositoryError>> + Send;
}
