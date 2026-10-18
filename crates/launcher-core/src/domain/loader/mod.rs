//! Mod loaders and their versions.

mod ports;

pub use ports::{
    LoaderInstallError, LoaderInstaller, LoaderMetadata, LoaderMetadataError, profile_id,
};

use std::fmt;

use thiserror::Error;

use crate::domain::instance::GameVersion;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoaderKind {
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl LoaderKind {
    pub const ALL: [Self; 5] = [
        Self::Vanilla,
        Self::Fabric,
        Self::Quilt,
        Self::Forge,
        Self::NeoForge,
    ];

    /// Stable lowercase id used in config files and modpack manifests.
    pub fn id(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.id().eq_ignore_ascii_case(id))
    }
}

/// A loader release, e.g. Fabric `0.17.2` or NeoForge `21.1.77`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoaderVersion(String);

impl LoaderVersion {
    pub fn parse(raw: &str) -> Result<Self, LoaderError> {
        let valid = !raw.is_empty()
            && raw.len() <= 64
            && raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'));
        if valid {
            Ok(Self(raw.to_owned()))
        } else {
            Err(LoaderError::InvalidVersion(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LoaderVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LoaderError {
    #[error("invalid loader version {0:?}")]
    InvalidVersion(String),
    #[error("{0:?} needs a loader version")]
    MissingVersion(LoaderKind),
    #[error("vanilla has no loader version")]
    UnexpectedVersion,
}

/// The loader an instance runs: vanilla, or a modded loader at a version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoaderSpec {
    kind: LoaderKind,
    version: Option<LoaderVersion>,
}

impl LoaderSpec {
    pub fn vanilla() -> Self {
        Self {
            kind: LoaderKind::Vanilla,
            version: None,
        }
    }

    pub fn new(kind: LoaderKind, version: Option<LoaderVersion>) -> Result<Self, LoaderError> {
        match (kind, &version) {
            (LoaderKind::Vanilla, Some(_)) => Err(LoaderError::UnexpectedVersion),
            (LoaderKind::Vanilla, None) => Ok(Self::vanilla()),
            (kind, None) => Err(LoaderError::MissingVersion(kind)),
            (kind, Some(_)) => Ok(Self { kind, version }),
        }
    }

    pub fn kind(&self) -> LoaderKind {
        self.kind
    }

    pub fn version(&self) -> Option<&LoaderVersion> {
        self.version.as_ref()
    }
}

/// A loader version offered for a game version by a loader's meta API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderRelease {
    pub version: LoaderVersion,
    pub game_version: GameVersion,
    pub stable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(raw: &str) -> LoaderVersion {
        LoaderVersion::parse(raw).unwrap()
    }

    #[test]
    fn vanilla_has_no_version_and_modded_loaders_require_one() {
        assert_eq!(
            LoaderSpec::new(LoaderKind::Vanilla, None),
            Ok(LoaderSpec::vanilla())
        );
        assert_eq!(
            LoaderSpec::new(LoaderKind::Vanilla, Some(version("1.0"))),
            Err(LoaderError::UnexpectedVersion)
        );
        assert_eq!(
            LoaderSpec::new(LoaderKind::Forge, None),
            Err(LoaderError::MissingVersion(LoaderKind::Forge))
        );
        let fabric = LoaderSpec::new(LoaderKind::Fabric, Some(version("0.17.2"))).unwrap();
        assert_eq!(fabric.version().map(LoaderVersion::as_str), Some("0.17.2"));
    }

    #[test]
    fn loader_ids_round_trip() {
        for kind in LoaderKind::ALL {
            assert_eq!(LoaderKind::from_id(kind.id()), Some(kind));
        }
        assert_eq!(LoaderKind::from_id("NeoForge"), Some(LoaderKind::NeoForge));
        assert_eq!(LoaderKind::from_id("liteloader"), None);
    }

    #[test]
    fn loader_versions_reject_path_and_shell_characters() {
        for raw in ["", "1.0/../x", "1.0 beta", "$(rm)", &"9".repeat(65)] {
            assert!(LoaderVersion::parse(raw).is_err(), "{raw}");
        }
        assert!(LoaderVersion::parse("1.21.1-52.0.16+mc").is_ok());
    }
}
