use std::future::Future;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use super::{LoaderKind, LoaderRelease, LoaderSpec};
use crate::domain::instance::GameVersion;
use crate::domain::launch::{Artifact, VersionId};
use crate::domain::ports::ProgressReporter;

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
    #[error("an installer processor failed: {0}")]
    ProcessorFailed(String),
    #[error("an installer processor produced an unexpected file: {0}")]
    OutputMismatch(String),
    #[error("the loader installation was cancelled")]
    Cancelled,
}

/// What [`LoaderInstaller::install`] stored, and what [`LoaderInstaller::finish`]
/// needs downloaded first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderProfile {
    /// The version id the instance launches ([`profile_id`]'s answer).
    pub id: VersionId,
    /// Installer tools (Forge/NeoForge processors and their classpath), each
    /// with a checksum; downloaded and verified together with the game files.
    pub tools: Vec<Artifact>,
}

/// What the processors run against, once the game files and Java are in place.
#[derive(Debug, Clone, Copy)]
pub struct LoaderEnvironment<'a> {
    /// The Java executable that runs the processors.
    pub java: &'a Path,
    /// The vanilla client jar (`{MINECRAFT_JAR}`).
    pub client_jar: &'a Path,
}

/// Installs a loader in two phases.
///
/// `install` stores the loader's version profile (a version JSON that inherits
/// from vanilla) where the `VersionSource` finds it, every library carrying a
/// checksum, and lists the installer tools to download. Fabric and Quilt are
/// done here. `finish` runs once the game files, the tools and Java are in
/// place: Forge and NeoForge run their installer processors and verify the
/// outputs that declare a sha1; for the others it does nothing.
pub trait LoaderInstaller: Send + Sync {
    fn install(
        &self,
        loader: &LoaderSpec,
        game: &GameVersion,
    ) -> impl Future<Output = Result<LoaderProfile, LoaderInstallError>> + Send;

    fn finish<R: ProgressReporter>(
        &self,
        loader: &LoaderSpec,
        game: &GameVersion,
        environment: LoaderEnvironment<'_>,
        reporter: &R,
        cancel: &CancellationToken,
    ) -> impl Future<Output = Result<(), LoaderInstallError>> + Send;
}

/// A Java tool to run: `java -cp <classpath> <main_class> <args>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessorCommand {
    pub java: PathBuf,
    pub classpath: Vec<PathBuf>,
    pub main_class: String,
    pub args: Vec<String>,
    /// Where the tool runs.
    pub working_dir: PathBuf,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProcessorRunError {
    #[error("could not start the processor: {0}")]
    Spawn(String),
    /// The tool failed; `output` is the tail of what it printed.
    #[error("the processor exited with {code:?}: {output}")]
    Failed { code: Option<i32>, output: String },
    #[error("the processor was cancelled")]
    Cancelled,
}

/// Runs installer processors (short-lived Java tools, not the game).
pub trait ProcessorRunner: Send + Sync {
    fn run(
        &self,
        command: &ProcessorCommand,
        cancel: &CancellationToken,
    ) -> impl Future<Output = Result<(), ProcessorRunError>> + Send;
}

/// The version id an instance launches: the game version for vanilla, the
/// loader profile id otherwise, as each loader names it:
/// `fabric-loader-<loader>-<game>`, `quilt-loader-<loader>-<game>`,
/// `<game>-forge-<loader>` and `neoforge-<loader>`. Known without the network,
/// so an installed loader instance launches offline. `None` without a loader
/// version, or for ids too long to be safe.
pub fn profile_id(loader: &LoaderSpec, game: &GameVersion) -> Option<VersionId> {
    if loader.kind == LoaderKind::Vanilla {
        return Some(VersionId::from(game));
    }
    let version = loader.version.as_ref()?.as_str();
    let game = game.as_str();
    let id = match loader.kind {
        LoaderKind::Vanilla => return None,
        LoaderKind::Fabric => format!("fabric-loader-{version}-{game}"),
        LoaderKind::Quilt => format!("quilt-loader-{version}-{game}"),
        LoaderKind::Forge => format!("{game}-forge-{version}"),
        LoaderKind::NeoForge => format!("neoforge-{version}"),
    };
    VersionId::parse(&id).ok()
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
    fn names_forge_and_neoforge_profiles_like_their_installers() {
        assert_eq!(
            profile_id(&with(LoaderKind::Forge, "65.1.5"), &game("26.2"))
                .unwrap()
                .as_str(),
            "26.2-forge-65.1.5"
        );
        assert_eq!(
            profile_id(&with(LoaderKind::NeoForge, "26.2.0.93"), &game("26.2"))
                .unwrap()
                .as_str(),
            "neoforge-26.2.0.93"
        );
    }
}
