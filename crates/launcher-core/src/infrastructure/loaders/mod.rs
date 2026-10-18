//! Loader adapters. Fabric and Quilt resolve through their meta APIs, which
//! list loader releases per game version and serve a version JSON (a
//! "profile") that inherits from the vanilla version. Forge and NeoForge ship
//! an installer jar whose processors patch the game (see [`forge`]).
//!
//! [`LoaderInstallers`] implements the `LoaderInstaller` port for every
//! [`LoaderSpec`]: it stores the profile under `versions/<id>/<id>.json`,
//! where `MojangVersionSource` reads it, after giving every library a sha1,
//! and runs the Forge and NeoForge processors in `finish`.
//! [`MetaLoaderMetadata`] and [`ForgeLoaderMetadata`] implement the
//! `LoaderMetadata` port.

mod fabric;
pub mod forge;
mod meta;
mod quilt;

#[cfg(test)]
mod tests;

pub use fabric::FABRIC_META_URL;
pub use forge::ForgeLoaderMetadata;
pub use meta::MetaLoaderMetadata;
pub use quilt::QUILT_META_URL;

use crate::domain::instance::GameVersion;
use crate::domain::launch::{LauncherPaths, VersionId};
use tokio_util::sync::CancellationToken;

use crate::domain::loader::{
    LoaderEnvironment, LoaderInstallError, LoaderInstaller, LoaderKind, LoaderProfile, LoaderSpec,
    ProcessorRunner, profile_id,
};
use crate::domain::ports::{HttpClient, ProgressReporter};
use crate::infrastructure::process::TokioProcessorRunner;

/// Installs any loader this launcher supports; vanilla needs no profile.
/// `P` runs the Forge and NeoForge installer processors.
#[derive(Debug)]
pub struct LoaderInstallers<H, P = TokioProcessorRunner> {
    http: H,
    paths: LauncherPaths,
    runner: P,
}

impl<H, P> LoaderInstallers<H, P> {
    pub fn new(http: H, paths: LauncherPaths, runner: P) -> Self {
        Self {
            http,
            paths,
            runner,
        }
    }
}

impl<H: HttpClient, P: ProcessorRunner> LoaderInstaller for LoaderInstallers<H, P> {
    async fn install(
        &self,
        loader: &LoaderSpec,
        game: &GameVersion,
    ) -> Result<LoaderProfile, LoaderInstallError> {
        let api = match loader.kind() {
            LoaderKind::Vanilla => return Ok(profile(VersionId::from(game))),
            LoaderKind::Fabric => &fabric::API,
            LoaderKind::Quilt => &quilt::API,
            LoaderKind::Forge => {
                return forge::install(&self.http, &self.paths, &forge::FORGE, loader, game).await;
            }
            LoaderKind::NeoForge => {
                return forge::install(&self.http, &self.paths, &forge::NEOFORGE, loader, game)
                    .await;
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
        Ok(profile(id))
    }

    /// Forge and NeoForge run their installer processors; Fabric and Quilt
    /// profiles are complete after `install`.
    async fn finish<R: ProgressReporter>(
        &self,
        loader: &LoaderSpec,
        game: &GameVersion,
        environment: LoaderEnvironment<'_>,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> Result<(), LoaderInstallError> {
        let flavor = match loader.kind() {
            LoaderKind::Vanilla | LoaderKind::Fabric | LoaderKind::Quilt => return Ok(()),
            LoaderKind::Forge => &forge::FORGE,
            LoaderKind::NeoForge => &forge::NEOFORGE,
        };
        forge::finish(
            &self.paths,
            &self.runner,
            flavor,
            loader,
            game,
            environment,
            reporter,
            cancel,
        )
        .await
    }
}

/// A profile that needs no installer tools.
fn profile(id: VersionId) -> LoaderProfile {
    LoaderProfile {
        id,
        tools: Vec::new(),
    }
}
