//! Modpack sources (Modrinth, CurseForge, packwiz) and what they return.

pub mod curseforge;
pub mod mrpack;
pub mod packwiz;
mod ports;
pub mod sync;

pub use ports::{
    CurseForgeArchive, CurseForgeLookup, ModpackSource, ModpackSourceError, MrpackArchive,
    PackFileError, PackFileLocation, PackFiles, PackLocation, PackManifestStore, PackwizSource,
    StagedFile,
};

use crate::domain::instance::GameVersion;
use crate::domain::integrity::Checksum;
use crate::domain::loader::LoaderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModpackSourceKind {
    Modrinth,
    CurseForge,
    Packwiz,
}

/// Where an installed modpack came from, so it can be updated later.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModpackRef {
    pub source: ModpackSourceKind,
    /// Modrinth project id, CurseForge project id, or the packwiz `pack.toml` URL.
    pub project: String,
    /// The installed version (Modrinth version id, CurseForge file id, packwiz version).
    pub version: String,
}

/// A search query; `page` is zero-based.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModpackQuery {
    pub text: String,
    pub game_version: Option<GameVersion>,
    pub loader: Option<LoaderKind>,
    pub page: u32,
    pub page_size: u32,
}

impl ModpackQuery {
    pub const MAX_PAGE_SIZE: u32 = 50;

    /// Clamps the page size into `1..=MAX_PAGE_SIZE` (APIs reject larger pages).
    pub fn normalized(mut self) -> Self {
        self.page_size = self.page_size.clamp(1, Self::MAX_PAGE_SIZE);
        self.text = self.text.trim().to_owned();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModpackSummary {
    pub source: ModpackSourceKind,
    pub project: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: Option<u64>,
    /// The mod loaders the pack runs on, in `pack_loaders` order.
    pub loaders: Vec<LoaderKind>,
}

/// The modded loaders a catalog lists for a pack, once each, in a fixed order
/// (Vanilla is never listed: every pack needs a loader).
pub fn pack_loaders(listed: impl IntoIterator<Item = LoaderKind>) -> Vec<LoaderKind> {
    let listed: Vec<LoaderKind> = listed.into_iter().collect();
    [
        LoaderKind::Fabric,
        LoaderKind::Quilt,
        LoaderKind::Forge,
        LoaderKind::NeoForge,
    ]
    .into_iter()
    .filter(|loader| listed.contains(loader))
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModpackPage {
    pub items: Vec<ModpackSummary>,
    pub total: Option<u64>,
}

/// One installable release of a modpack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModpackVersion {
    pub id: String,
    pub name: String,
    /// The version label the author gave it (`14.1.1`).
    pub version_number: String,
    pub game_versions: Vec<GameVersion>,
    pub loaders: Vec<LoaderKind>,
    /// RFC 3339 publication time, when the catalog reports it.
    pub published: Option<String>,
    /// The pack archive to install, when the version has one this launcher reads.
    pub pack_file: Option<ModpackFile>,
}

/// A downloadable pack archive and the hash it is verified against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModpackFile {
    pub url: String,
    pub filename: String,
    pub size: u64,
    pub checksum: Checksum,
}

#[cfg(test)]
mod tests {

    #[test]
    fn pack_loaders_lists_each_modded_loader_once_in_a_fixed_order() {
        use crate::domain::loader::LoaderKind::{Fabric, Forge, NeoForge, Quilt, Vanilla};
        assert_eq!(
            super::pack_loaders([NeoForge, Vanilla, Fabric, NeoForge, Forge, Quilt, Fabric]),
            [Fabric, Quilt, Forge, NeoForge]
        );
        assert!(super::pack_loaders([Vanilla]).is_empty());
    }

    use super::*;

    #[test]
    fn queries_clamp_the_page_size_and_trim_text() {
        let query = ModpackQuery {
            text: "  fabulously optimized ".into(),
            page_size: 500,
            ..Default::default()
        }
        .normalized();
        assert_eq!(query.page_size, ModpackQuery::MAX_PAGE_SIZE);
        assert_eq!(query.text, "fabulously optimized");
        assert_eq!(ModpackQuery::default().normalized().page_size, 1);
    }
}
