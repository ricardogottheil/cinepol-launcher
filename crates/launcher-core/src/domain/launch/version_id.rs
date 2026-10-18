//! The id of a version JSON: a Mojang release (`26.2`) or a loader profile
//! (`quilt-loader-0.20.0-beta.9-1.21.11-pre1`). It names a cache directory.

use std::fmt;

use thiserror::Error;

use super::RelativePath;
use crate::domain::instance::GameVersion;

const MAX_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("not a valid version id: {0:?}")]
pub struct VersionIdError(pub String);

/// One safe path segment of `[A-Za-z0-9._+-]` and inner spaces (Mojang's old
/// pre-release ids such as `1.14 Pre-Release 1`), up to 64 characters: the
/// same rules as `GameVersion`, with room for a loader prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VersionId(String);

impl VersionId {
    pub fn parse(raw: &str) -> Result<Self, VersionIdError> {
        let valid = !raw.is_empty()
            && raw.len() <= MAX_LEN
            && !raw.starts_with(['.', ' '])
            && !raw.ends_with(['.', ' '])
            && !raw.contains("..")
            && raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-' | ' '))
            && RelativePath::parse(raw).is_ok();
        if valid {
            Ok(Self(raw.to_owned()))
        } else {
            Err(VersionIdError(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&GameVersion> for VersionId {
    fn from(version: &GameVersion) -> Self {
        // GameVersion's rules are these rules with a shorter limit, so every
        // game version is a valid version id (tested below).
        Self(version.as_str().to_owned())
    }
}

impl fmt::Display for VersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_releases_snapshots_and_long_loader_profiles() {
        for id in [
            "26.2",
            "1.21.11-pre1",
            "26w14a",
            "fabric-loader-0.20.0-26.2",
            "quilt-loader-0.20.0-beta.9-1.21.11-pre1",
            "1.20.1-forge-47.2.0",
            "neoforge-21.1.77",
            "1.14 Pre-Release 1",
            "3D Shareware v1.34",
            "fabric-loader-0.16.0-1.14 Pre-Release 5",
        ] {
            assert_eq!(VersionId::parse(id).unwrap().as_str(), id);
        }
    }

    #[test]
    fn rejects_anything_that_is_not_one_safe_segment() {
        for id in [
            "",
            ".",
            "..",
            ".hidden",
            "a/b",
            "a\\b",
            "../26.2",
            "C:26.2",
            "26.2 ",
            " 26.2",
            "26.2.",
            "1..2",
            "x\0",
            &"a".repeat(65),
        ] {
            assert!(VersionId::parse(id).is_err(), "{id:?}");
        }
    }

    #[test]
    fn every_game_version_is_a_valid_version_id() {
        for raw in [
            "26.2",
            "1.14 Pre-Release 1",
            "3D Shareware v1.34",
            "b1.7.3",
            "rd-132211",
        ] {
            let game = GameVersion::parse(raw).unwrap();
            assert_eq!(VersionId::from(&game), VersionId::parse(raw).unwrap());
        }
    }
}
