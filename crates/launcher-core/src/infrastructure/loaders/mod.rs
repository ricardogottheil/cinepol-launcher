//! Loader adapters. Fabric and Quilt resolve through their meta APIs, which
//! list loader releases per game version and serve a version JSON (a
//! "profile") that inherits from the vanilla version.
//!
//! [`LoaderInstallers`] implements the `LoaderInstaller` port for every
//! [`LoaderSpec`]: it stores the profile under `versions/<id>/<id>.json`,
//! where `MojangVersionSource` reads it, after giving every library a sha1.
//! [`MetaLoaderMetadata`] implements the `LoaderMetadata` port.

mod fabric;
mod meta;
mod quilt;

#[cfg(test)]
mod tests;

pub use fabric::FABRIC_META_URL;
pub use meta::MetaLoaderMetadata;
pub use quilt::QUILT_META_URL;

use crate::domain::instance::GameVersion;
use crate::domain::launch::{LauncherPaths, VersionId};
use crate::domain::loader::{
    LoaderInstallError, LoaderInstaller, LoaderKind, LoaderSpec, profile_id,
};
use crate::domain::ports::HttpClient;

/// Installs any loader this launcher supports; vanilla needs no profile.
#[derive(Debug)]
pub struct LoaderInstallers<H> {
    http: H,
    paths: LauncherPaths,
}

impl<H> LoaderInstallers<H> {
    pub fn new(http: H, paths: LauncherPaths) -> Self {
        Self { http, paths }
    }
}

impl<H: HttpClient> LoaderInstaller for LoaderInstallers<H> {
    async fn install(
        &self,
        loader: &LoaderSpec,
        game: &GameVersion,
    ) -> Result<VersionId, LoaderInstallError> {
        let api = match loader.kind() {
            LoaderKind::Vanilla => return Ok(VersionId::from(game)),
            LoaderKind::Fabric => &fabric::API,
            LoaderKind::Quilt => &quilt::API,
            kind @ (LoaderKind::Forge | LoaderKind::NeoForge) => {
                return Err(LoaderInstallError::UnsupportedLoader(kind));
            }
        };
        let version = loader.version().ok_or_else(|| {
            LoaderInstallError::Invalid(format!("{:?} needs a loader version", api.kind))
        })?;
        let id = profile_id(loader, game).ok_or_else(|| {
            LoaderInstallError::Invalid(format!(
                "{:?} {version} for Minecraft {} has no safe profile id",
                api.kind,
                game.as_str()
            ))
        })?;
        meta::install_profile(&self.http, &self.paths, api, version, game, &id).await?;
        Ok(id)
    }
}
