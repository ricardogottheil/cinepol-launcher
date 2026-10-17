use std::future::Future;

use thiserror::Error;

use super::ServerList;
use crate::domain::instance::InstanceId;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServerListStoreError {
    #[error("servers.dat is corrupt: {0}")]
    Corrupt(String),
    #[error("servers.dat could not be read or written: {0}")]
    Io(String),
}

/// Reads and writes an instance's `servers.dat`. A missing file is an empty list.
pub trait ServerListStore: Send + Sync {
    fn load(
        &self,
        instance: &InstanceId,
    ) -> impl Future<Output = Result<ServerList, ServerListStoreError>> + Send;
    fn save(
        &self,
        instance: &InstanceId,
        list: &ServerList,
    ) -> impl Future<Output = Result<(), ServerListStoreError>> + Send;
}
