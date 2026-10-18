//! The releases of a loader for a game version, newest first, so the UI can
//! offer the latest stable one by default.

use std::cmp::Ordering;

use thiserror::Error;

use crate::domain::instance::{GameVersion, GameVersionError};
use crate::domain::loader::{LoaderMetadata, LoaderMetadataError, LoaderRelease};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ListLoaderVersionsError {
    #[error(transparent)]
    InvalidGameVersion(#[from] GameVersionError),
    #[error(transparent)]
    Metadata(#[from] LoaderMetadataError),
}

pub async fn list_loader_versions<M: LoaderMetadata>(
    game_version: &str,
    metadata: &M,
) -> Result<Vec<LoaderRelease>, ListLoaderVersionsError> {
    let game = GameVersion::parse(game_version.trim())?;
    let mut releases = metadata.releases(&game).await?;
    releases.sort_by(|a, b| compare_loader_versions(b.version.as_str(), a.version.as_str()));
    Ok(releases)
}

/// Numeric parts compare as numbers (`beta.10 > beta.9`), a final release
/// sorts above its own pre-releases (`0.20.0 > 0.20.0-beta.10`), and build
/// metadata after `+` is ignored, like semantic versioning.
fn compare_loader_versions(a: &str, b: &str) -> Ordering {
    let split = |raw: &str| {
        let raw = raw.split_once('+').map_or(raw, |(version, _)| version);
        match raw.split_once('-') {
            Some((base, pre)) => (base.to_owned(), Some(pre.to_owned())),
            None => (raw.to_owned(), None),
        }
    };
    let (base_a, pre_a) = split(a);
    let (base_b, pre_b) = split(b);
    compare_parts(&base_a, &base_b).then_with(|| match (pre_a, pre_b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => compare_parts(&x, &y),
    })
}

/// Dot- or dash-separated parts: numbers by value, anything else as text,
/// and a number below a word (`1 < beta`), as semantic versioning does.
fn compare_parts(a: &str, b: &str) -> Ordering {
    let parts = |raw: &str| raw.split(['.', '-']).map(str::to_owned).collect::<Vec<_>>();
    let (left, right) = (parts(a), parts(b));
    for (x, y) in left.iter().zip(&right) {
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::loader::{LoaderKind, LoaderVersion};
    use crate::testing::FakeLoaderMetadata;

    fn release(version: &str, stable: bool) -> LoaderRelease {
        LoaderRelease {
            version: LoaderVersion::parse(version).unwrap(),
            game_version: GameVersion::parse("26.2").unwrap(),
            stable,
        }
    }

    fn versions(releases: &[LoaderRelease]) -> Vec<&str> {
        releases.iter().map(|r| r.version.as_str()).collect()
    }

    #[tokio::test]
    async fn lists_newest_first_with_releases_above_their_betas() {
        let metadata = FakeLoaderMetadata {
            kind: LoaderKind::Quilt,
            releases: vec![
                release("0.20.0-beta.9", false),
                release("0.24.0", true),
                release("0.20.0-beta.10", false),
                release("0.20.0", true),
                release("0.19.5", true),
            ],
        };
        let listed = list_loader_versions("26.2", &metadata).await.unwrap();
        assert_eq!(
            versions(&listed),
            [
                "0.24.0",
                "0.20.0",
                "0.20.0-beta.10",
                "0.20.0-beta.9",
                "0.19.5"
            ]
        );
    }

    #[tokio::test]
    async fn rejects_an_invalid_game_version_and_passes_metadata_errors_through() {
        let metadata = FakeLoaderMetadata {
            kind: LoaderKind::Fabric,
            releases: vec![release("0.20.0", false)],
        };
        assert!(matches!(
            list_loader_versions("../26.2", &metadata).await,
            Err(ListLoaderVersionsError::InvalidGameVersion(_))
        ));
        assert!(matches!(
            list_loader_versions("1.8.9", &metadata).await,
            Err(ListLoaderVersionsError::Metadata(
                LoaderMetadataError::UnsupportedGameVersion(LoaderKind::Fabric, _)
            ))
        ));
    }
}
