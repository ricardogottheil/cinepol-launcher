//! Modrinth API v2 answers, as far as the launcher reads them. Unknown
//! fields are ignored; optional data may be missing or `null`.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

/// `GET /search`.
#[derive(Debug, Deserialize)]
pub(super) struct SearchResponse {
    pub hits: Vec<SearchHit>,
    #[serde(default)]
    pub total_hits: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SearchHit {
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: Option<u64>,
    /// Tags, loaders among them ("fabric", "optimization"...).
    #[serde(default)]
    pub categories: Vec<String>,
}

/// `GET /project/{id}`.
#[derive(Debug, Deserialize)]
pub(super) struct Project {
    pub id: String,
    pub title: String,
    pub project_type: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: Option<u64>,
    #[serde(default)]
    pub loaders: Vec<String>,
}

/// One entry of `GET /project/{id}/version`, `GET /version/{id}`, and the
/// values of `POST /version_files`.
#[derive(Debug, Deserialize)]
pub(super) struct Version {
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    /// `release`, `beta` or `alpha`.
    #[serde(default)]
    pub version_type: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version_number: Option<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub date_published: Option<String>,
    #[serde(default)]
    pub files: Vec<VersionFile>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Dependency {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// `required`, `optional`, `incompatible` or `embedded`.
    pub dependency_type: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct VersionFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
    /// Algorithm name (`sha1`, `sha512`) to hex digest.
    #[serde(default)]
    pub hashes: BTreeMap<String, String>,
}

/// `POST /version_files` answer: each known hash to the version holding it.
pub(super) type VersionFilesResponse = HashMap<String, VersionFilesEntry>;

/// The part of a version `POST /version_files` needs; the rest is ignored.
#[derive(Debug, Deserialize)]
pub(super) struct VersionFilesEntry {
    #[serde(default)]
    pub files: Vec<VersionFile>,
}

/// `POST /version_files` request body.
#[derive(Debug, Serialize)]
pub(super) struct VersionFilesRequest<'a> {
    pub hashes: &'a [String],
    pub algorithm: &'static str,
}
