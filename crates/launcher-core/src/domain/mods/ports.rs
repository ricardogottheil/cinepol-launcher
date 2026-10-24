use std::collections::HashMap;
use std::future::Future;

use super::{ModPage, ModQuery, ModRegistry, ModSummary, ModTarget, ModVersion};
use crate::domain::instance::InstanceId;
use crate::domain::modpack::{ModpackSourceError, PackFileError};

/// A catalog of individual mods. It shares the modpack catalogs' errors.
pub trait ModSource: Send + Sync {
    /// Mods that have a version for the query's target.
    fn search(
        &self,
        query: &ModQuery,
    ) -> impl Future<Output = Result<ModPage, ModpackSourceError>> + Send;
    /// A project's versions that fit `target`, newest first.
    fn versions(
        &self,
        project: &str,
        target: &ModTarget,
    ) -> impl Future<Output = Result<Vec<ModVersion>, ModpackSourceError>> + Send;
    /// One version by id, whatever it was built for.
    fn version(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<ModVersion, ModpackSourceError>> + Send;
    /// The projects with these ids; unknown ones are left out.
    fn projects(
        &self,
        ids: &[String],
    ) -> impl Future<Output = Result<Vec<ModSummary>, ModpackSourceError>> + Send;
    /// The versions whose jar has one of these sha512 digests, by digest;
    /// digests the catalog does not know are left out.
    fn identify(
        &self,
        sha512: &[String],
    ) -> impl Future<Output = Result<HashMap<String, ModVersion>, ModpackSourceError>> + Send;
}

/// The per-instance record of the mods the launcher installed.
pub trait ModRegistryStore: Send + Sync {
    /// The saved registry, or an empty one when the instance has none.
    fn load(
        &self,
        id: &InstanceId,
    ) -> impl Future<Output = Result<ModRegistry, PackFileError>> + Send;
    fn save(
        &self,
        id: &InstanceId,
        registry: &ModRegistry,
    ) -> impl Future<Output = Result<(), PackFileError>> + Send;
}
