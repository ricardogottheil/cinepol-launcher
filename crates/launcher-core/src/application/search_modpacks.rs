//! Searches a modpack catalog.

use crate::domain::modpack::{ModpackPage, ModpackQuery, ModpackSource, ModpackSourceError};

/// The query is trimmed and its page size clamped before it reaches the catalog.
pub async fn search_modpacks<C: ModpackSource>(
    source: &C,
    query: ModpackQuery,
) -> Result<ModpackPage, ModpackSourceError> {
    source.search(&query.normalized()).await
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::collections::HashMap;

    use crate::domain::modpack::{ModpackPage, ModpackSourceKind, ModpackSummary};
    use crate::testing::FakeModpackSource;

    pub fn summary(project: &str, title: &str) -> ModpackSummary {
        ModpackSummary {
            source: ModpackSourceKind::Modrinth,
            project: project.into(),
            title: title.into(),
            description: format!("{title} description"),
            icon_url: None,
            downloads: Some(10),
            loaders: Vec::new(),
        }
    }

    /// A Modrinth catalog with two packs and no versions.
    pub fn catalog() -> FakeModpackSource {
        FakeModpackSource {
            kind: ModpackSourceKind::Modrinth,
            page: ModpackPage {
                items: vec![
                    summary("fo", "Fabulously Optimized"),
                    summary("cobble", "Cobblemon Official"),
                ],
                total: Some(2),
            },
            versions: HashMap::new(),
            known: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn trims_the_query_before_searching() {
        let source = fixtures::catalog();
        let query = ModpackQuery {
            text: "  fabulously ".into(),
            page_size: 500,
            ..Default::default()
        };
        let page = search_modpacks(&source, query).await.unwrap();
        let titles: Vec<&str> = page.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["Fabulously Optimized"]);
        assert_eq!(page.total, Some(2));
    }
}
