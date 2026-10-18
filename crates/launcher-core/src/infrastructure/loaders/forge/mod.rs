//! Forge and NeoForge: both ship an installer jar holding an
//! `install_profile.json` (spec 1), a `version.json` that inherits from
//! vanilla, the files their processors read (`data/`) and sometimes local
//! libraries (`maven/`). They differ only in where they live and how their
//! versions are listed, so one [`Flavor`] per loader drives the shared code.
//!
//! - [`ForgeLoaderMetadata`] lists releases (Forge maven metadata plus its
//!   promotions, the NeoForge maven API).
//! - [`install`] downloads and verifies the installer, stores `version.json`
//!   and returns the processor tools to download.
//! - [`finish`] runs the client processors once the game, the tools and Java
//!   are in place, verifies their outputs and skips work already done.

mod installer;
mod manifest;
mod profile;
mod versions;

#[cfg(test)]
mod tests;

pub(super) use installer::{finish, install};
pub use versions::ForgeLoaderMetadata;

use crate::domain::instance::GameVersion;
use crate::domain::loader::{LoaderKind, LoaderVersion};

/// Forge's maven repository.
pub const FORGE_MAVEN_URL: &str = "https://maven.minecraftforge.net";
/// Every Forge version, as `<game>-<loader>`.
pub const FORGE_METADATA_URL: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
/// The latest and recommended Forge per game version.
pub const FORGE_PROMOTIONS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
/// NeoForge's maven repository.
pub const NEOFORGE_MAVEN_URL: &str = "https://maven.neoforged.net/releases";
/// Every NeoForge version, as JSON `{"versions": [...]}`.
pub const NEOFORGE_VERSIONS_URL: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

/// Where one loader's installers live.
#[derive(Debug)]
pub(super) struct Flavor {
    pub kind: LoaderKind,
    /// The maven repository, without a trailing slash.
    pub repository: &'static str,
    pub group: &'static str,
    pub artifact: &'static str,
    /// Forge versions its artifacts `<game>-<loader>`; NeoForge just `<loader>`.
    pub game_prefixed: bool,
}

impl Flavor {
    /// The maven version of the loader's artifacts.
    pub fn maven_version(&self, game: &GameVersion, loader: &LoaderVersion) -> String {
        if self.game_prefixed {
            format!("{}-{}", game.as_str(), loader.as_str())
        } else {
            loader.as_str().to_owned()
        }
    }
}

pub(super) const FORGE: Flavor = Flavor {
    kind: LoaderKind::Forge,
    repository: FORGE_MAVEN_URL,
    group: "net.minecraftforge",
    artifact: "forge",
    game_prefixed: true,
};

pub(super) const NEOFORGE: Flavor = Flavor {
    kind: LoaderKind::NeoForge,
    repository: NEOFORGE_MAVEN_URL,
    group: "net.neoforged",
    artifact: "neoforge",
    game_prefixed: false,
};
