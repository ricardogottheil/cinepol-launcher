//! Both mod catalogs behind one [`ModSource`]: a search goes to the catalog
//! the player picked, and every other request to the catalog its ids belong
//! to ([`mod_catalog`]). Dependencies, updates and titles therefore stay
//! with the catalog each mod came from.

use std::collections::HashMap;

use crate::domain::modpack::{ModpackSourceError, ModpackSourceKind};
use crate::domain::mods::{
    ModPage, ModQuery, ModSource, ModSummary, ModTarget, ModVersion, mod_catalog,
};

/// Modrinth and CurseForge, routed by id.
#[derive(Debug)]
pub struct ModCatalogs<'a, M, C> {
    modrinth: &'a M,
    curseforge: &'a C,
}

impl<'a, M, C> ModCatalogs<'a, M, C> {
    pub fn new(modrinth: &'a M, curseforge: &'a C) -> Self {
        Self {
            modrinth,
            curseforge,
        }
    }
}

impl<M: ModSource, C: ModSource> ModSource for ModCatalogs<'_, M, C> {
    async fn search(&self, query: &ModQuery) -> Result<ModPage, ModpackSourceError> {
        match query.catalog {
            ModpackSourceKind::Modrinth => self.modrinth.search(query).await,
            ModpackSourceKind::CurseForge => self.curseforge.search(query).await,
            // packwiz is a pack format, not a catalog of mods.
            ModpackSourceKind::Packwiz => Ok(ModPage {
                items: Vec::new(),
                total: Some(0),
            }),
        }
    }

    async fn versions(
        &self,
        project: &str,
        target: &ModTarget,
    ) -> Result<Vec<ModVersion>, ModpackSourceError> {
        match mod_catalog(project) {
            ModpackSourceKind::CurseForge => self.curseforge.versions(project, target).await,
            _ => self.modrinth.versions(project, target).await,
        }
    }

    async fn version(&self, id: &str) -> Result<ModVersion, ModpackSourceError> {
        match mod_catalog(id) {
            ModpackSourceKind::CurseForge => self.curseforge.version(id).await,
            _ => self.modrinth.version(id).await,
        }
    }

    async fn projects(&self, ids: &[String]) -> Result<Vec<ModSummary>, ModpackSourceError> {
        let (curseforge, modrinth): (Vec<String>, Vec<String>) = ids
            .iter()
            .cloned()
            .partition(|id| mod_catalog(id) == ModpackSourceKind::CurseForge);
        let mut projects = Vec::with_capacity(ids.len());
        if !modrinth.is_empty() {
            projects.extend(self.modrinth.projects(&modrinth).await?);
        }
        if !curseforge.is_empty() {
            projects.extend(self.curseforge.projects(&curseforge).await?);
        }
        Ok(projects)
    }

    /// Only Modrinth identifies jars by sha512.
    async fn identify(
        &self,
        sha512: &[String],
    ) -> Result<HashMap<String, ModVersion>, ModpackSourceError> {
        self.modrinth.identify(sha512).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::domain::instance::GameVersion;
    use crate::domain::loader::LoaderKind;
    use crate::testing::{FakeModSource, mod_jar, mod_version, sha512_hex};

    fn target() -> ModTarget {
        ModTarget {
            game_version: GameVersion::parse("1.21.1").unwrap(),
            loader: LoaderKind::Fabric,
        }
    }

    fn catalogs() -> (FakeModSource, FakeModSource) {
        let modrinth = FakeModSource::default().with(
            "sodium",
            "Sodium",
            vec![mod_version(
                "sodium",
                "sodium-2",
                "1.21.1",
                &[LoaderKind::Fabric],
            )],
        );
        let curseforge = FakeModSource::default().with(
            "curseforge:394468",
            "Sodium (CurseForge)",
            vec![mod_version(
                "curseforge:394468",
                "curseforge:6000",
                "1.21.1",
                &[LoaderKind::Fabric],
            )],
        );
        (modrinth, curseforge)
    }

    #[tokio::test]
    async fn a_search_goes_to_the_catalog_the_player_picked() {
        let (modrinth, curseforge) = catalogs();
        let catalogs = ModCatalogs::new(&modrinth, &curseforge);
        let query = |catalog| ModQuery {
            text: "sodium".into(),
            target: target(),
            catalog,
            page: 0,
            page_size: 20,
        };
        catalogs
            .search(&query(ModpackSourceKind::CurseForge))
            .await
            .unwrap();
        assert_eq!(curseforge.searched().len(), 1);
        assert!(modrinth.searched().is_empty());
        catalogs
            .search(&query(ModpackSourceKind::Modrinth))
            .await
            .unwrap();
        assert_eq!(modrinth.searched().len(), 1);
        let packwiz = catalogs
            .search(&query(ModpackSourceKind::Packwiz))
            .await
            .unwrap();
        assert!(packwiz.items.is_empty());
        assert_eq!(
            (modrinth.searched().len(), curseforge.searched().len()),
            (1, 1)
        );
    }

    #[tokio::test]
    async fn every_id_is_answered_by_its_own_catalog() {
        let (modrinth, curseforge) = catalogs();
        let catalogs = ModCatalogs::new(&modrinth, &curseforge);

        let from_curseforge = catalogs
            .versions("curseforge:394468", &target())
            .await
            .unwrap();
        assert_eq!(from_curseforge[0].id, "curseforge:6000");
        let from_modrinth = catalogs.versions("sodium", &target()).await.unwrap();
        assert_eq!(from_modrinth[0].id, "sodium-2");
        assert_eq!(curseforge.asked(), ["curseforge:394468"]);
        assert_eq!(modrinth.asked(), ["sodium"]);

        assert_eq!(
            catalogs.version("curseforge:6000").await.unwrap().project,
            "curseforge:394468"
        );
        assert_eq!(
            catalogs.version("sodium-2").await.unwrap().project,
            "sodium"
        );

        let titles: Vec<String> = catalogs
            .projects(&["curseforge:394468".into(), "sodium".into()])
            .await
            .unwrap()
            .into_iter()
            .map(|summary| summary.title)
            .collect();
        assert_eq!(titles, ["Sodium", "Sodium (CurseForge)"]);

        // Loose jars are identified by Modrinth only.
        let hash = sha512_hex(&mod_jar("sodium-2"));
        assert_eq!(catalogs.identify(&[hash]).await.unwrap().len(), 1);
        let jar_of_curseforge = sha512_hex(&mod_jar("curseforge:6000"));
        assert!(
            catalogs
                .identify(&[jar_of_curseforge])
                .await
                .unwrap()
                .is_empty()
        );
    }
}
