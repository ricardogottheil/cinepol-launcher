//! Modrinth API v2 (<https://docs.modrinth.com/api/>): modpack search,
//! project and version details, and the files the catalog knows by hash.
//!
//! Requests go through the launcher's [`HttpClient`], which sends the
//! descriptive User-Agent Modrinth asks for and waits out `Retry-After` on
//! HTTP 429. Every parameter is percent-encoded and project ids are checked
//! before they become part of a URL.

mod wire;

use std::collections::{HashMap, HashSet};

use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::RelativePath;
use crate::domain::loader::LoaderKind;
use crate::domain::modpack::mrpack::is_allowed_download;
use crate::domain::modpack::{
    ModpackFile, ModpackPage, ModpackQuery, ModpackSource, ModpackSourceError, ModpackSourceKind,
    ModpackSummary, ModpackVersion, pack_loaders,
};
use crate::domain::ports::{HttpClient, HttpError};

/// The public Modrinth API.
pub const MODRINTH_API: &str = "https://api.modrinth.com/v2";

/// Hashes per `POST /version_files` request.
const KNOWN_FILES_BATCH: usize = 500;

const MODPACK_TYPE: &str = "modpack";
const PACK_EXTENSION: &str = ".mrpack";

/// The Modrinth catalog of modpacks.
#[derive(Debug, Clone)]
pub struct ModrinthSource<H> {
    http: H,
    base_url: String,
}

impl<H> ModrinthSource<H> {
    pub fn new(http: H) -> Self {
        Self::with_base_url(http, MODRINTH_API)
    }

    /// Talks to another API root (tests, a staging server); no trailing slash.
    pub fn with_base_url(http: H, base_url: impl Into<String>) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }
}

impl<H: HttpClient> ModrinthSource<H> {
    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        subject: &str,
    ) -> Result<T, ModpackSourceError> {
        let url = format!("{}{path}", self.base_url);
        let body = self
            .http
            .get_bytes(&url)
            .await
            .map_err(|error| source_error(error, subject))?;
        parse(&body)
    }
}

impl<H: HttpClient> ModpackSource for ModrinthSource<H> {
    fn kind(&self) -> ModpackSourceKind {
        ModpackSourceKind::Modrinth
    }

    async fn search(&self, query: &ModpackQuery) -> Result<ModpackPage, ModpackSourceError> {
        let query = query.clone().normalized();
        let mut facets = vec![vec![format!("project_type:{MODPACK_TYPE}")]];
        if let Some(game) = &query.game_version {
            facets.push(vec![format!("versions:{}", game.as_str())]);
        }
        // Vanilla packs carry no loader category.
        if let Some(loader) = query.loader.filter(|kind| *kind != LoaderKind::Vanilla) {
            facets.push(vec![format!("categories:{}", loader.id())]);
        }
        let facets = serde_json::to_string(&facets)
            .map_err(|error| ModpackSourceError::InvalidResponse(error.to_string()))?;
        let offset = u64::from(query.page) * u64::from(query.page_size);
        let path = format!(
            "/search?query={}&facets={}&index=relevance&offset={offset}&limit={}",
            encode(&query.text),
            encode(&facets),
            query.page_size,
        );
        let answer: wire::SearchResponse = self.get(&path, "search").await?;
        Ok(ModpackPage {
            items: answer
                .hits
                .into_iter()
                .map(|hit| ModpackSummary {
                    source: ModpackSourceKind::Modrinth,
                    project: hit.project_id,
                    title: hit.title,
                    description: hit.description.unwrap_or_default(),
                    icon_url: hit.icon_url,
                    downloads: hit.downloads,
                    loaders: listed_loaders(&hit.categories),
                })
                .collect(),
            total: answer.total_hits,
        })
    }

    async fn project(&self, project: &str) -> Result<ModpackSummary, ModpackSourceError> {
        let id = checked_id(project)?;
        let answer: wire::Project = self.get(&format!("/project/{id}"), project).await?;
        if answer.project_type != MODPACK_TYPE {
            return Err(ModpackSourceError::NotFound(project.to_owned()));
        }
        Ok(ModpackSummary {
            source: ModpackSourceKind::Modrinth,
            project: answer.id,
            title: answer.title,
            description: answer.description.unwrap_or_default(),
            icon_url: answer.icon_url,
            downloads: answer.downloads,
            loaders: listed_loaders(&answer.loaders),
        })
    }

    async fn versions(&self, project: &str) -> Result<Vec<ModpackVersion>, ModpackSourceError> {
        let id = checked_id(project)?;
        let answer: Vec<wire::Version> =
            self.get(&format!("/project/{id}/version"), project).await?;
        let mut versions: Vec<(Option<String>, ModpackVersion)> = answer
            .into_iter()
            .map(|version| {
                let key = version.date_published.as_deref().and_then(publication_key);
                (key, modpack_version(version))
            })
            .collect();
        // Newest first; undated versions last, in the order the API gave them.
        versions.sort_by(|(a, _), (b, _)| b.cmp(a));
        Ok(versions.into_iter().map(|(_, version)| version).collect())
    }

    async fn known_files(
        &self,
        sha512: &[String],
    ) -> Result<HashMap<String, Vec<String>>, ModpackSourceError> {
        // Valid digests only, lowercased, each once, in the order asked.
        let mut seen = HashSet::new();
        let hashes: Vec<String> = sha512
            .iter()
            .filter_map(|hash| Checksum::parse(HashAlgorithm::Sha512, hash).ok())
            .map(|checksum| checksum.value().to_owned())
            .filter(|hash| seen.insert(hash.clone()))
            .collect();
        let url = format!("{}/version_files", self.base_url);
        let mut known = HashMap::new();
        for batch in hashes.chunks(KNOWN_FILES_BATCH) {
            let body = serde_json::to_vec(&wire::VersionFilesRequest {
                hashes: batch,
                algorithm: "sha512",
            })
            .map_err(|error| ModpackSourceError::InvalidResponse(error.to_string()))?;
            let answer = self
                .http
                .post_json(&url, body)
                .await
                .map_err(|error| source_error(error, "version_files"))?;
            let mut answer: wire::VersionFilesResponse = parse(&answer)?;
            for hash in batch {
                let Some(version) = answer.remove(hash) else {
                    continue;
                };
                let urls: Vec<String> = version
                    .files
                    .into_iter()
                    .filter(|file| {
                        file.hashes
                            .get("sha512")
                            .is_some_and(|value| value.eq_ignore_ascii_case(hash))
                    })
                    .map(|file| file.url)
                    .filter(|url| is_allowed_download(url))
                    .collect();
                if !urls.is_empty() {
                    known.insert(hash.clone(), urls);
                }
            }
        }
        Ok(known)
    }
}

fn modpack_version(version: wire::Version) -> ModpackVersion {
    let pack_file = pack_file(&version.files);
    ModpackVersion {
        name: version.name.unwrap_or_else(|| version.id.clone()),
        version_number: version.version_number.unwrap_or_else(|| version.id.clone()),
        id: version.id,
        game_versions: version
            .game_versions
            .iter()
            .filter_map(|raw| GameVersion::parse(raw).ok())
            .collect(),
        loaders: version
            .loaders
            .iter()
            .filter_map(|raw| LoaderKind::from_id(raw))
            .filter(|kind| *kind != LoaderKind::Vanilla)
            .collect(),
        published: version.date_published,
        pack_file,
    }
}

/// The primary `.mrpack` file, else the first one; `None` when it lacks a
/// valid sha512, is not on an allowed download host, or its name is not a
/// single safe file name.
fn pack_file(files: &[wire::VersionFile]) -> Option<ModpackFile> {
    let is_pack =
        |file: &&wire::VersionFile| file.filename.to_ascii_lowercase().ends_with(PACK_EXTENSION);
    let file = files
        .iter()
        .filter(is_pack)
        .find(|file| file.primary)
        .or_else(|| files.iter().find(is_pack))?;
    let checksum = Checksum::parse(HashAlgorithm::Sha512, file.hashes.get("sha512")?).ok()?;
    let safe_name = RelativePath::parse(&file.filename).is_ok() && !file.filename.contains('/');
    if !safe_name || !is_allowed_download(&file.url) {
        return None;
    }
    Some(ModpackFile {
        url: file.url.clone(),
        filename: file.filename.clone(),
        size: file.size,
        checksum,
    })
}

/// A sortable form of an RFC 3339 UTC time (`2026-10-11T10:32:15.321598Z`):
/// the seconds plus the fraction padded to nanoseconds. Modrinth reports UTC.
fn publication_key(raw: &str) -> Option<String> {
    let seconds = raw.get(..19)?;
    let rest = raw.get(19..)?;
    let fraction: String = rest
        .strip_prefix('.')
        .unwrap_or_default()
        .chars()
        .take_while(char::is_ascii_digit)
        .take(9)
        .collect();
    Some(format!("{seconds}.{fraction:0<9}"))
}

/// Modrinth ids are 8 base62 characters and slugs are short words; anything
/// else is refused before it becomes part of a URL.
fn checked_id(project: &str) -> Result<&str, ModpackSourceError> {
    let valid = (1..=64).contains(&project.len())
        && project
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(project)
    } else {
        Err(ModpackSourceError::NotFound(project.to_owned()))
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ModpackSourceError> {
    serde_json::from_slice(body)
        .map_err(|error| ModpackSourceError::InvalidResponse(error.to_string()))
}

fn source_error(error: HttpError, subject: &str) -> ModpackSourceError {
    match error {
        HttpError::NotFound(_) => ModpackSourceError::NotFound(subject.to_owned()),
        HttpError::RateLimited {
            retry_after_secs, ..
        } => ModpackSourceError::RateLimited { retry_after_secs },
        other @ (HttpError::Status { .. }
        | HttpError::Network { .. }
        | HttpError::TooLarge { .. }) => ModpackSourceError::Network(other.to_string()),
    }
}

/// Percent-encodes a query value: RFC 3986 unreserved bytes stay, everything
/// else (spaces included) becomes `%XX`.
pub(crate) fn encode(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Modrinth tags hold the loaders by id among other tags.
fn listed_loaders(tags: &[String]) -> Vec<LoaderKind> {
    pack_loaders(tags.iter().filter_map(|tag| LoaderKind::from_id(tag)))
}

#[cfg(test)]
mod tests;
