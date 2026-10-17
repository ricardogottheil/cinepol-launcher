use std::future::Future;

use thiserror::Error;

use super::{ModpackPage, ModpackQuery, ModpackSourceKind, ModpackVersion};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModpackSourceError {
    #[error("{0:?} is not configured (missing API key)")]
    NotConfigured(ModpackSourceKind),
    #[error("modpack {0} was not found")]
    NotFound(String),
    #[error("rate limited; retry after {retry_after_secs} s")]
    RateLimited { retry_after_secs: u64 },
    #[error("modpack request failed: {0}")]
    Network(String),
    #[error("unexpected response: {0}")]
    InvalidResponse(String),
}

/// A browsable modpack catalog. Installing a version is a separate use case
/// built on top (Milestones 10-12).
pub trait ModpackSource: Send + Sync {
    fn kind(&self) -> ModpackSourceKind;
    fn search(
        &self,
        query: &ModpackQuery,
    ) -> impl Future<Output = Result<ModpackPage, ModpackSourceError>> + Send;
    fn versions(
        &self,
        project: &str,
    ) -> impl Future<Output = Result<Vec<ModpackVersion>, ModpackSourceError>> + Send;
}
