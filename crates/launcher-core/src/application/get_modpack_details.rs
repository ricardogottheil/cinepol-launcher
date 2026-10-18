//! One modpack's details and its installable versions.

use crate::domain::modpack::{ModpackSource, ModpackSourceError, ModpackSummary, ModpackVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModpackDetails {
    pub project: ModpackSummary,
    /// Newest first, as the catalog returns them.
    pub versions: Vec<ModpackVersion>,
}

/// Fetches the project and its versions concurrently.
pub async fn get_modpack_details<C: ModpackSource>(
    source: &C,
    project: &str,
) -> Result<ModpackDetails, ModpackSourceError> {
    let (project, versions) = tokio::try_join!(source.project(project), source.versions(project))?;
    Ok(ModpackDetails { project, versions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::search_modpacks::fixtures::{catalog, summary};

    fn version(id: &str) -> ModpackVersion {
        ModpackVersion {
            id: id.into(),
            name: format!("Release {id}"),
            version_number: id.into(),
            game_versions: vec![],
            loaders: vec![],
            published: None,
            pack_file: None,
        }
    }

    #[tokio::test]
    async fn returns_the_project_with_its_versions() {
        let mut source = catalog();
        source
            .versions
            .insert("fo".into(), vec![version("v2"), version("v1")]);
        let details = get_modpack_details(&source, "fo").await.unwrap();
        assert_eq!(details.project, summary("fo", "Fabulously Optimized"));
        assert_eq!(details.versions, [version("v2"), version("v1")]);
    }

    #[tokio::test]
    async fn an_unknown_project_is_not_found() {
        let source = catalog();
        assert_eq!(
            get_modpack_details(&source, "missing").await,
            Err(ModpackSourceError::NotFound("missing".into()))
        );
    }
}
