//! Searches the mod catalog for an instance: only mods with a version for
//! its game version and loader come back, so the player cannot pick one
//! that will not run.

use thiserror::Error;

use crate::domain::instance::{InstanceId, InstanceRepository, InstanceRepositoryError};
use crate::domain::modpack::{ModpackSourceError, ModpackSourceKind};
use crate::domain::mods::{ModPage, ModQuery, ModSource, ModTarget};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SearchModsError {
    #[error(transparent)]
    Instance(#[from] InstanceRepositoryError),
    #[error(transparent)]
    Source(#[from] ModpackSourceError),
}

/// One page of mods for the instance from `catalog`. The text is trimmed
/// and the page size clamped before the catalog sees them.
pub async fn search_mods<I: InstanceRepository, S: ModSource>(
    instances: &I,
    source: &S,
    id: &InstanceId,
    catalog: ModpackSourceKind,
    text: String,
    page: u32,
    page_size: u32,
) -> Result<ModPage, SearchModsError> {
    let instance = instances.get(id).await?;
    let query = ModQuery {
        text,
        target: ModTarget {
            game_version: instance.game_version.clone(),
            loader: instance.loader.kind(),
        },
        catalog,
        page,
        page_size,
    };
    Ok(source.search(&query.normalized()).await?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::testing::{FakeModSource, InMemoryInstanceRepository, fabric_instance};

    #[tokio::test]
    async fn searches_for_the_instance_and_reports_unknown_ones() {
        let instances = InMemoryInstanceRepository::default();
        let id = InstanceId::parse("survival").unwrap();
        fabric_instance(&instances, &id).await;
        let source = FakeModSource::default();
        let page = search_mods(
            &instances,
            &source,
            &id,
            ModpackSourceKind::CurseForge,
            "  iris ".into(),
            0,
            500,
        )
        .await
        .unwrap();
        let asked = source.searched();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].text, "iris");
        assert_eq!(asked[0].target.game_version.as_str(), "1.21.1");
        assert_eq!(
            asked[0].target.loader,
            crate::domain::loader::LoaderKind::Fabric
        );
        assert_eq!(asked[0].catalog, ModpackSourceKind::CurseForge);
        assert_eq!(asked[0].page_size, 50);
        assert_eq!(page.total, Some(0));

        search_mods(
            &instances,
            &source,
            &id,
            ModpackSourceKind::CurseForge,
            String::new(),
            0,
            0,
        )
        .await
        .unwrap();
        assert_eq!(source.searched()[1].page_size, 1);

        let other = InstanceId::parse("other").unwrap();
        assert!(matches!(
            search_mods(
                &instances,
                &source,
                &other,
                ModpackSourceKind::Modrinth,
                String::new(),
                0,
                20
            )
            .await,
            Err(SearchModsError::Instance(_))
        ));
    }
}
