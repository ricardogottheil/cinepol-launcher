use std::future::Future;

use super::{ModPage, ModQuery, ModSummary, ModTarget, ModVersion};
use crate::domain::modpack::ModpackSourceError;

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
}
