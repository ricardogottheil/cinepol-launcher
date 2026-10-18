//! Forge and NeoForge release lists.
//!
//! - Forge: its maven metadata lists every version as `<game>-<loader>`; its
//!   promotions name the recommended release per game version. A release is
//!   stable when it is at or below the recommended one; without a
//!   recommended release, every release is.
//! - NeoForge: its maven API lists every version. Releases for Minecraft
//!   `1.<minor>[.<patch>]` start with `<minor>.<patch>.`; for the year-based
//!   `<year>.<drop>[.<hotfix>]` they start with `<year>.<drop>.<hotfix>.`
//!   (hotfix 0 when absent). Pre-releases carry a `-beta` (or `-alpha`)
//!   suffix; builds for snapshots carry `+<snapshot>` and are never listed.
//!
//! Releases are returned newest first.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::Deserialize;

use super::{
    FORGE, FORGE_METADATA_URL, FORGE_PROMOTIONS_URL, Flavor, NEOFORGE, NEOFORGE_VERSIONS_URL,
};
use crate::domain::instance::GameVersion;
use crate::domain::loader::{
    LoaderKind, LoaderMetadata, LoaderMetadataError, LoaderRelease, LoaderVersion,
};
use crate::domain::ports::HttpClient;

/// The `LoaderMetadata` port for Forge or NeoForge.
#[derive(Debug)]
pub struct ForgeLoaderMetadata<H> {
    http: H,
    flavor: &'static Flavor,
}

impl<H> ForgeLoaderMetadata<H> {
    pub fn forge(http: H) -> Self {
        Self {
            http,
            flavor: &FORGE,
        }
    }

    pub fn neoforge(http: H) -> Self {
        Self {
            http,
            flavor: &NEOFORGE,
        }
    }
}

impl<H: HttpClient> LoaderMetadata for ForgeLoaderMetadata<H> {
    fn kind(&self) -> LoaderKind {
        self.flavor.kind
    }

    async fn releases(
        &self,
        game: &GameVersion,
    ) -> Result<Vec<LoaderRelease>, LoaderMetadataError> {
        let fetch = |url: &'static str| async move {
            self.http
                .get_bytes(url)
                .await
                .map_err(|error| LoaderMetadataError::Network(error.to_string()))
        };
        match self.flavor.kind {
            LoaderKind::NeoForge => {
                parse_neoforge_releases(game, &fetch(NEOFORGE_VERSIONS_URL).await?)
            }
            _ => {
                let metadata = fetch(FORGE_METADATA_URL).await?;
                let promotions = fetch(FORGE_PROMOTIONS_URL).await?;
                parse_forge_releases(game, &metadata, &promotions)
            }
        }
    }
}

fn unsupported(kind: LoaderKind, game: &GameVersion) -> LoaderMetadataError {
    LoaderMetadataError::UnsupportedGameVersion(kind, game.as_str().to_owned())
}

#[derive(Debug, Deserialize)]
struct Promotions {
    #[serde(default)]
    promos: BTreeMap<String, String>,
}

/// Every `<version>` inside `<versions>` (the whole document when that block
/// is missing). Plain string scanning: the format is flat and fixed.
fn maven_versions(xml: &str) -> impl Iterator<Item = &str> {
    let block = xml
        .split_once("<versions>")
        .and_then(|(_, rest)| rest.split_once("</versions>"))
        .map_or(xml, |(block, _)| block);
    block.split("<version>").skip(1).filter_map(|chunk| {
        chunk
            .split_once("</version>")
            .map(|(version, _)| version.trim())
    })
}

pub(super) fn parse_forge_releases(
    game: &GameVersion,
    metadata: &[u8],
    promotions: &[u8],
) -> Result<Vec<LoaderRelease>, LoaderMetadataError> {
    let xml = std::str::from_utf8(metadata).map_err(|error| {
        LoaderMetadataError::Network(format!("invalid Forge maven metadata: {error}"))
    })?;
    let promotions: Promotions = serde_json::from_slice(promotions).map_err(|error| {
        LoaderMetadataError::Network(format!("invalid Forge promotions: {error}"))
    })?;
    let recommended = promotions
        .promos
        .get(&format!("{}-recommended", game.as_str()))
        .map(|version| version_key(version));
    let prefix = format!("{}-", game.as_str());
    let releases = maven_versions(xml)
        .filter_map(|raw| raw.strip_prefix(prefix.as_str()))
        .filter_map(|raw| LoaderVersion::parse(raw).ok())
        .map(|version| LoaderRelease {
            stable: recommended
                .as_ref()
                .is_none_or(|limit| version_key(version.as_str()) <= *limit),
            version,
            game_version: game.clone(),
        })
        .collect();
    newest_first(LoaderKind::Forge, game, releases)
}

#[derive(Debug, Deserialize)]
struct NeoForgeVersions {
    versions: Vec<String>,
}

/// The prefix of the NeoForge versions for a Minecraft release.
fn neoforge_prefix(game: &str) -> Option<String> {
    let parts: Vec<&str> = game.split('.').collect();
    if !parts
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    match parts.as_slice() {
        ["1", minor] => Some(format!("{minor}.0.")),
        ["1", minor, patch] => Some(format!("{minor}.{patch}.")),
        [year, drop] => Some(format!("{year}.{drop}.0.")),
        [year, drop, hotfix] => Some(format!("{year}.{drop}.{hotfix}.")),
        _ => None,
    }
}

pub(super) fn parse_neoforge_releases(
    game: &GameVersion,
    bytes: &[u8],
) -> Result<Vec<LoaderRelease>, LoaderMetadataError> {
    let listed: NeoForgeVersions = serde_json::from_slice(bytes).map_err(|error| {
        LoaderMetadataError::Network(format!("invalid NeoForge version list: {error}"))
    })?;
    let prefix =
        neoforge_prefix(game.as_str()).ok_or_else(|| unsupported(LoaderKind::NeoForge, game))?;
    let releases = listed
        .versions
        .iter()
        .filter(|raw| raw.starts_with(prefix.as_str()) && !raw.contains('+'))
        .filter_map(|raw| LoaderVersion::parse(raw).ok())
        .map(|version| LoaderRelease {
            stable: !version.as_str().contains('-'),
            version,
            game_version: game.clone(),
        })
        .collect();
    newest_first(LoaderKind::NeoForge, game, releases)
}

/// The numeric parts of a version up to its first `-` or `+`.
fn version_key(raw: &str) -> Vec<u64> {
    raw.split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// Newest first; a release sorts above the pre-releases of the same number.
fn compare(a: &LoaderRelease, b: &LoaderRelease) -> Ordering {
    version_key(b.version.as_str())
        .cmp(&version_key(a.version.as_str()))
        .then(b.stable.cmp(&a.stable))
        .then_with(|| b.version.as_str().cmp(a.version.as_str()))
}

fn newest_first(
    kind: LoaderKind,
    game: &GameVersion,
    mut releases: Vec<LoaderRelease>,
) -> Result<Vec<LoaderRelease>, LoaderMetadataError> {
    releases.sort_by(compare);
    releases.dedup_by(|a, b| a.version == b.version);
    if releases.is_empty() {
        return Err(unsupported(kind, game));
    }
    Ok(releases)
}
