//! The meta API shared by Fabric and Quilt: `<base>/<game>` lists loader
//! releases and `<base>/<game>/<loader>/profile/json` serves the profile.

use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::{LauncherPaths, MavenCoordinate, VersionId};
use crate::domain::loader::{
    LoaderInstallError, LoaderKind, LoaderMetadata, LoaderMetadataError, LoaderRelease,
    LoaderVersion,
};
use crate::domain::ports::{HttpClient, HttpError};
use crate::infrastructure::fs::write_atomic;
use crate::infrastructure::mojang::LIBRARIES_BASE_URL;

/// One loader's meta API.
#[derive(Debug)]
pub(super) struct MetaApi {
    pub kind: LoaderKind,
    /// `.../versions/loader`, without a trailing slash.
    pub base_url: &'static str,
}

impl MetaApi {
    fn releases_url(&self, game: &GameVersion) -> String {
        format!("{}/{}", self.base_url, url_segment(game.as_str()))
    }

    fn profile_url(&self, game: &GameVersion, loader: &LoaderVersion) -> String {
        format!(
            "{}/{}/{}/profile/json",
            self.base_url,
            url_segment(game.as_str()),
            url_segment(loader.as_str())
        )
    }
}

/// Game and loader versions are already restricted to `[A-Za-z0-9._+-]` plus
/// inner spaces (old pre-release names); only the spaces need escaping.
fn url_segment(raw: &str) -> String {
    raw.replace(' ', "%20")
}

/// Both APIs answer an unknown game or loader version with 400 (Fabric) or
/// 404 (Quilt).
fn is_unknown(error: &HttpError) -> bool {
    matches!(
        error,
        HttpError::NotFound(_) | HttpError::Status { status: 400, .. }
    )
}

/// The `LoaderMetadata` port over Fabric's or Quilt's meta API.
#[derive(Debug)]
pub struct MetaLoaderMetadata<H> {
    http: H,
    api: &'static MetaApi,
}

impl<H> MetaLoaderMetadata<H> {
    pub fn fabric(http: H) -> Self {
        Self {
            http,
            api: &super::fabric::API,
        }
    }

    pub fn quilt(http: H) -> Self {
        Self {
            http,
            api: &super::quilt::API,
        }
    }
}

impl<H: HttpClient> LoaderMetadata for MetaLoaderMetadata<H> {
    fn kind(&self) -> LoaderKind {
        self.api.kind
    }

    /// Releases in the order the API lists them (Fabric: newest first; Quilt
    /// mostly newest first, without a strict order).
    async fn releases(
        &self,
        game: &GameVersion,
    ) -> Result<Vec<LoaderRelease>, LoaderMetadataError> {
        match self.http.get_bytes(&self.api.releases_url(game)).await {
            Ok(bytes) => parse_releases(self.api.kind, game, &bytes),
            Err(error) if is_unknown(&error) => Err(LoaderMetadataError::UnsupportedGameVersion(
                self.api.kind,
                game.as_str().to_owned(),
            )),
            Err(error) => Err(LoaderMetadataError::Network(error.to_string())),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawRelease {
    loader: RawLoader,
}

#[derive(Debug, Deserialize)]
struct RawLoader {
    version: String,
    #[serde(default)]
    stable: Option<bool>,
}

/// Parses a release list. Entries with unsafe versions are skipped; an empty
/// list means the loader does not support that game version.
pub(super) fn parse_releases(
    kind: LoaderKind,
    game: &GameVersion,
    bytes: &[u8],
) -> Result<Vec<LoaderRelease>, LoaderMetadataError> {
    let raw: Vec<RawRelease> = serde_json::from_slice(bytes).map_err(|error| {
        LoaderMetadataError::Network(format!("invalid {kind:?} release list: {error}"))
    })?;
    let releases: Vec<LoaderRelease> = raw
        .into_iter()
        .filter_map(|entry| {
            let version = LoaderVersion::parse(&entry.loader.version)
                .inspect_err(|error| tracing::debug!(%error, "skipping a loader release"))
                .ok()?;
            // Quilt has no `stable` flag; its pre-releases carry a `-beta.N` suffix.
            let stable = entry
                .loader
                .stable
                .unwrap_or_else(|| !version.as_str().contains('-'));
            Some(LoaderRelease {
                version,
                game_version: game.clone(),
                stable,
            })
        })
        .collect();
    if releases.is_empty() {
        return Err(LoaderMetadataError::UnsupportedGameVersion(
            kind,
            game.as_str().to_owned(),
        ));
    }
    Ok(releases)
}

/// The sha1 in a maven `.sha1` sidecar: the first token (some sidecars append
/// the file name), 40 hex characters, lowercased.
pub(super) fn parse_sidecar(body: &[u8]) -> Option<String> {
    let token = std::str::from_utf8(body).ok()?.split_whitespace().next()?;
    let sha1 = token.to_ascii_lowercase();
    Checksum::parse(HashAlgorithm::Sha1, &sha1).ok()?;
    Some(sha1)
}

/// Fetches the profile, gives every library a sha1 and stores it atomically
/// at `versions/<id>/<id>.json`. When the network fails, a valid cached
/// profile keeps the instance launchable.
pub(super) async fn install_profile<H: HttpClient>(
    http: &H,
    paths: &LauncherPaths,
    api: &MetaApi,
    loader: &LoaderVersion,
    game: &GameVersion,
    id: &VersionId,
) -> Result<(), LoaderInstallError> {
    let path = paths
        .version_json(id.as_str())
        .ok_or_else(|| LoaderInstallError::Invalid(format!("unsafe profile id {id}")))?;
    let profile = match fetch_profile(http, api, loader, game, id).await {
        Ok(profile) => profile,
        Err(LoaderInstallError::Network(reason)) => {
            if cached_profile_is_valid(&path, id, game).await {
                tracing::warn!(%id, %reason, "loader meta unreachable; keeping the cached profile");
                return Ok(());
            }
            return Err(LoaderInstallError::Network(reason));
        }
        Err(error) => return Err(error),
    };
    let bytes = serde_json::to_vec_pretty(&profile)
        .map_err(|error| LoaderInstallError::Storage(error.to_string()))?;
    write_atomic(&path, &bytes)
        .await
        .map_err(|error| LoaderInstallError::Storage(format!("{}: {error}", path.display())))
}

async fn fetch_profile<H: HttpClient>(
    http: &H,
    api: &MetaApi,
    loader: &LoaderVersion,
    game: &GameVersion,
    id: &VersionId,
) -> Result<Value, LoaderInstallError> {
    let bytes = http
        .get_bytes(&api.profile_url(game, loader))
        .await
        .map_err(|error| {
            if is_unknown(&error) {
                LoaderInstallError::UnknownLoaderVersion(
                    api.kind,
                    loader.as_str().to_owned(),
                    game.as_str().to_owned(),
                )
            } else {
                LoaderInstallError::Network(error.to_string())
            }
        })?;
    let mut profile: Value = serde_json::from_slice(&bytes)
        .map_err(|error| LoaderInstallError::Invalid(error.to_string()))?;
    check_header(&profile, id, game)?;

    let mut filled = Vec::new();
    for (index, library) in libraries(&profile)?.into_iter().enumerate() {
        if !declares_sha1(library)? {
            filled.push((index, fetch_sidecar(http, library).await?));
        }
    }
    if let Some(Value::Array(list)) = profile.get_mut("libraries") {
        for (index, sha1) in filled {
            if let Some(Value::Object(library)) = list.get_mut(index) {
                library.insert("sha1".to_owned(), Value::String(sha1));
            }
        }
    }

    // Never cache a profile with an unhashed library.
    every_library_hashed(&profile)?;
    Ok(profile)
}

/// Fails unless every library of a profile declares a well-formed sha1.
pub(super) fn every_library_hashed(profile: &Value) -> Result<(), LoaderInstallError> {
    for library in libraries(profile)? {
        if !declares_sha1(library)? {
            return Err(LoaderInstallError::Invalid(format!(
                "library {} has no sha1",
                library_name(library)
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ProfileHeader {
    id: String,
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
}

/// Fails unless the profile is `id` and inherits from `game`.
pub(super) fn check_header(
    profile: &Value,
    id: &VersionId,
    game: &GameVersion,
) -> Result<(), LoaderInstallError> {
    let header = ProfileHeader::deserialize(profile)
        .map_err(|error| LoaderInstallError::Invalid(error.to_string()))?;
    if header.id != id.as_str() {
        return Err(LoaderInstallError::Invalid(format!(
            "expected profile {id}, got {:?}",
            header.id
        )));
    }
    if header.inherits_from.as_deref() != Some(game.as_str()) {
        return Err(LoaderInstallError::Invalid(format!(
            "profile {id} inherits from {:?}, not {}",
            header.inherits_from,
            game.as_str()
        )));
    }
    Ok(())
}

fn libraries(profile: &Value) -> Result<Vec<&Map<String, Value>>, LoaderInstallError> {
    match profile.get("libraries") {
        None => Ok(Vec::new()),
        Some(Value::Array(list)) => list
            .iter()
            .map(|library| {
                library
                    .as_object()
                    .ok_or_else(|| LoaderInstallError::Invalid("a library is not an object".into()))
            })
            .collect(),
        Some(_) => Err(LoaderInstallError::Invalid(
            "libraries is not a list".into(),
        )),
    }
}

fn library_name(library: &Map<String, Value>) -> &str {
    library
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
}

/// Whether the library declares a well-formed sha1. A malformed one is an
/// error. A library with a `downloads` block is read from there, so it must
/// declare its sha1 there; a sidecar could not help it.
fn declares_sha1(library: &Map<String, Value>) -> Result<bool, LoaderInstallError> {
    let invalid = |what: &str| {
        LoaderInstallError::Invalid(format!("library {}: {what}", library_name(library)))
    };
    let declared = match library.get("downloads") {
        Some(downloads) => match downloads.get("artifact").and_then(|a| a.get("sha1")) {
            Some(sha1) => Some(sha1),
            None => return Err(invalid("its downloads declare no sha1")),
        },
        None => library.get("sha1").filter(|sha1| !sha1.is_null()),
    };
    match declared {
        None => Ok(false),
        Some(Value::String(raw)) => Checksum::parse(HashAlgorithm::Sha1, raw)
            .map(|_| true)
            .map_err(|error| invalid(&format!("bad sha1 ({error})"))),
        Some(_) => Err(invalid("sha1 is not a string")),
    }
}

async fn fetch_sidecar<H: HttpClient>(
    http: &H,
    library: &Map<String, Value>,
) -> Result<String, LoaderInstallError> {
    let name = library_name(library);
    let coordinate = MavenCoordinate::parse(name)
        .map_err(|error| LoaderInstallError::Invalid(format!("library {name}: {error}")))?;
    let base = library
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or(LIBRARIES_BASE_URL);
    let separator = if base.ends_with('/') { "" } else { "/" };
    let url = format!("{base}{separator}{}.sha1", coordinate.path().as_str());
    let body = http.get_bytes(&url).await.map_err(|error| match error {
        HttpError::NotFound(_)
        | HttpError::Status {
            status: 400..=499, ..
        } => LoaderInstallError::Invalid(format!(
            "library {name} declares no sha1 and its maven has none"
        )),
        other => LoaderInstallError::Network(other.to_string()),
    })?;
    parse_sidecar(&body)
        .ok_or_else(|| LoaderInstallError::Invalid(format!("library {name}: malformed maven sha1")))
}

/// A cached profile is valid when it is the expected profile and every
/// library declares a well-formed sha1.
async fn cached_profile_is_valid(path: &Path, id: &VersionId, game: &GameVersion) -> bool {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return false;
    };
    let Ok(profile) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    check_header(&profile, id, game).is_ok()
        && libraries(&profile).is_ok_and(|list| {
            list.into_iter()
                .all(|library| matches!(declares_sha1(library), Ok(true)))
        })
}
