use std::future::Future;

use thiserror::Error;

use super::{LoaderKind, LoaderRelease, LoaderSpec};
use crate::domain::instance::GameVersion;
use crate::domain::launch::VersionId;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LoaderMetadataError {
    #[error("{0:?} does not support Minecraft {1}")]
    UnsupportedGameVersion(LoaderKind, String),
    #[error("loader metadata request failed: {0}")]
    Network(String),
}

/// Lists the releases a loader offers for a game version (Fabric/Quilt meta,
/// Forge/NeoForge maven metadata). Installing is a separate port (Milestones 8-9).
pub trait LoaderMetadata: Send + Sync {
    fn kind(&self) -> LoaderKind;
    fn releases(
        &self,
        game_version: &GameVersion,
    ) -> impl Future<Output = Result<Vec<LoaderRelease>, LoaderMetadataError>> + Send;
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum LoaderInstallError {
    #[error("{0:?} does not support Minecraft {1}")]
    UnsupportedGameVersion(LoaderKind, String),
    #[error("{0:?} {1} does not exist for Minecraft {2}")]
    UnknownLoaderVersion(LoaderKind, String, String),
    #[error("{0:?} cannot be installed by this launcher yet")]
    UnsupportedLoader(LoaderKind),
    #[error("loader profile request failed: {0}")]
    Network(String),
    #[error("the loader profile is invalid: {0}")]
    Invalid(String),
    #[error("could not store the loader profile: {0}")]
    Storage(String),
}

/// Installs a loader's version profile: fetches it (a version JSON that
/// inherits from vanilla), gives every library a checksum (from the profile or
/// the maven `.sha1` next to the jar) and stores it where the `VersionSource`
/// finds it. Returns the profile id, which is always [`profile_id`]'s answer.
pub trait LoaderInstaller: Send + Sync {
    fn install(
        &self,
        loader: &LoaderSpec,
        game: &GameVersion,
    ) -> impl Future<Output = Result<VersionId, LoaderInstallError>> + Send;
}

/// The version id an instance launches: the game version for vanilla, the
/// loader profile id otherwise (`fabric-loader-<loader>-<game>`,
/// `quilt-loader-<loader>-<game>`, as the meta APIs name them). Known without
/// the network, so an installed loader instance launches offline. `None` for
/// loaders this launcher cannot install yet, or ids too long to be safe.
pub fn profile_id(loader: &LoaderSpec, game: &GameVersion) -> Option<VersionId> {
    let prefix = match loader.kind {
        LoaderKind::Vanilla => return Some(VersionId::from(game)),
        LoaderKind::Fabric => "fabric-loader",
        LoaderKind::Quilt => "quilt-loader",
        LoaderKind::Forge | LoaderKind::NeoForge => return None,
    };
    let version = loader.version.as_ref()?;
    VersionId::parse(&format!("{prefix}-{}-{}", version.as_str(), game.as_str())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::loader::LoaderVersion;

    fn game(raw: &str) -> GameVersion {
        GameVersion::parse(raw).unwrap()
    }

    fn with(kind: LoaderKind, version: &str) -> LoaderSpec {
        LoaderSpec::new(kind, Some(LoaderVersion::parse(version).unwrap())).unwrap()
    }

    #[test]
    fn names_profiles_like_the_meta_apis() {
        assert_eq!(
            profile_id(&LoaderSpec::vanilla(), &game("26.2"))
                .unwrap()
                .as_str(),
            "26.2"
        );
        assert_eq!(
            profile_id(&with(LoaderKind::Fabric, "0.20.0"), &game("26.2"))
                .unwrap()
                .as_str(),
            "fabric-loader-0.20.0-26.2"
        );
        assert_eq!(
            profile_id(
                &with(LoaderKind::Quilt, "0.20.0-beta.9"),
                &game("1.21.11-pre1")
            )
            .unwrap()
            .as_str(),
            "quilt-loader-0.20.0-beta.9-1.21.11-pre1"
        );
    }

    #[test]
    fn forge_and_neoforge_have_no_profile_yet() {
        assert_eq!(
            profile_id(&with(LoaderKind::Forge, "47.2.0"), &game("1.20.1")),
            None
        );
        assert_eq!(
            profile_id(&with(LoaderKind::NeoForge, "21.1.77"), &game("1.21.1")),
            None
        );
    }
}
