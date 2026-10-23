//! CurseForge API v1 (<https://docs.curseforge.com/rest-api/>): modpack
//! search, project and file details, and the files and projects a pack's
//! manifest lists.
//!
//! Every request carries the launcher's API key (`x-api-key`), which is a
//! build-time setting; without one the source reports itself as not
//! configured and sends nothing. Requests go through the launcher's
//! [`HttpClient`] (User-Agent, retries, `Retry-After`).

mod wire;

#[cfg(test)]
mod tests;

use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::loader::LoaderKind;
use crate::domain::modpack::curseforge::{
    CurseForgeFile, CurseForgeProject, RawFileRef, RawLoader, RawManifest, is_allowed_download,
};
use crate::domain::modpack::{
    CurseForgeLookup, ModpackFile, ModpackPage, ModpackQuery, ModpackSource, ModpackSourceError,
    ModpackSourceKind, ModpackSummary, ModpackVersion, PackFileError, pack_loaders,
};
use crate::domain::ports::{ApiKeyHeader, HttpClient, HttpError, Secret};
use crate::infrastructure::modrinth::encode;

/// The public CurseForge API.
pub const CURSEFORGE_API: &str = "https://api.curseforge.com";
/// The header CurseForge reads the key from.
pub const API_KEY_HEADER: &str = "x-api-key";

const MINECRAFT_GAME_ID: u32 = 432;
const MODPACK_CLASS_ID: u64 = 4471;
/// Ids per batch request.
const BATCH: usize = 200;
/// Files listed per project; the newest come first.
const FILES_PAGE: u32 = 50;

/// The CurseForge catalog of modpacks.
#[derive(Debug, Clone)]
pub struct CurseForgeSource<H> {
    http: H,
    key: Option<ApiKeyHeader>,
    base_url: String,
}

impl<H> CurseForgeSource<H> {
    /// `key` is `None` (or blank) in builds without a CurseForge key.
    pub fn new(http: H, key: Option<&str>) -> Self {
        Self::with_base_url(http, key, CURSEFORGE_API)
    }

    /// Talks to another API root (tests); no trailing slash.
    pub fn with_base_url(http: H, key: Option<&str>, base_url: impl Into<String>) -> Self {
        Self {
            http,
            key: key
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(|key| ApiKeyHeader::new(API_KEY_HEADER, Secret::new(key))),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    /// Whether this build can talk to CurseForge.
    pub fn is_configured(&self) -> bool {
        self.key.is_some()
    }

    fn key(&self) -> Result<&ApiKeyHeader, ModpackSourceError> {
        self.key.as_ref().ok_or(ModpackSourceError::NotConfigured(
            ModpackSourceKind::CurseForge,
        ))
    }
}

impl<H: HttpClient> CurseForgeSource<H> {
    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        subject: &str,
    ) -> Result<T, ModpackSourceError> {
        let key = self.key()?;
        let url = format!("{}{path}", self.base_url);
        let body = self
            .http
            .get_bytes_keyed(&url, key)
            .await
            .map_err(|error| source_error(error, subject))?;
        parse(&body)
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<T, ModpackSourceError> {
        let key = self.key()?;
        let url = format!("{}{path}", self.base_url);
        let body = serde_json::to_vec(body)
            .map_err(|error| ModpackSourceError::InvalidResponse(error.to_string()))?;
        let answer = self
            .http
            .post_json_keyed(&url, body, key)
            .await
            .map_err(|error| source_error(error, path))?;
        parse(&answer)
    }
}

impl<H: HttpClient> ModpackSource for CurseForgeSource<H> {
    fn kind(&self) -> ModpackSourceKind {
        ModpackSourceKind::CurseForge
    }

    async fn search(&self, query: &ModpackQuery) -> Result<ModpackPage, ModpackSourceError> {
        let query = query.clone().normalized();
        let index = u64::from(query.page) * u64::from(query.page_size);
        // Sorted by popularity, like the CurseForge site.
        let mut path = format!(
            "/v1/mods/search?gameId={MINECRAFT_GAME_ID}&classId={MODPACK_CLASS_ID}\
             &searchFilter={}&sortField=2&sortOrder=desc&index={index}&pageSize={}",
            encode(&query.text),
            query.page_size,
        );
        if let Some(game) = &query.game_version {
            path.push_str(&format!("&gameVersion={}", encode(game.as_str())));
        }
        if let Some(loader) = query.loader.and_then(loader_type) {
            path.push_str(&format!("&modLoaderType={loader}"));
        }
        let answer: wire::SearchResponse = self.get(&path, "search").await?;
        Ok(ModpackPage {
            items: answer.data.into_iter().map(summary).collect(),
            total: answer.pagination.and_then(|page| page.total_count),
        })
    }

    async fn project(&self, project: &str) -> Result<ModpackSummary, ModpackSourceError> {
        let id = checked_id(project)?;
        let answer: wire::Data<wire::Mod> = self.get(&format!("/v1/mods/{id}"), project).await?;
        if answer.data.class_id != Some(MODPACK_CLASS_ID) {
            return Err(ModpackSourceError::NotFound(project.to_owned()));
        }
        Ok(summary(answer.data))
    }

    async fn versions(&self, project: &str) -> Result<Vec<ModpackVersion>, ModpackSourceError> {
        let id = checked_id(project)?;
        let answer: wire::Data<Vec<wire::File>> = self
            .get(
                &format!("/v1/mods/{id}/files?pageSize={FILES_PAGE}"),
                project,
            )
            .await?;
        let mut versions: Vec<ModpackVersion> = answer
            .data
            .into_iter()
            .filter(|file| file.is_server_pack != Some(true))
            .map(version)
            .collect();
        // Newest first, whatever order CurseForge used.
        versions.sort_by(|a, b| b.published.cmp(&a.published));
        Ok(versions)
    }

    /// CurseForge knows files by fingerprint, not sha512.
    async fn known_files(
        &self,
        _sha512: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<String>>, ModpackSourceError> {
        Ok(std::collections::HashMap::new())
    }
}

impl<H: HttpClient> CurseForgeLookup for CurseForgeSource<H> {
    async fn files(&self, ids: &[u64]) -> Result<Vec<CurseForgeFile>, ModpackSourceError> {
        let mut found = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BATCH) {
            let answer: wire::Data<Vec<wire::File>> = self
                .post("/v1/mods/files", &wire::FilesRequest { file_ids: chunk })
                .await?;
            found.extend(answer.data.into_iter().map(file));
        }
        Ok(found)
    }

    async fn projects(&self, ids: &[u64]) -> Result<Vec<CurseForgeProject>, ModpackSourceError> {
        let mut found = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BATCH) {
            let answer: wire::Data<Vec<wire::Mod>> = self
                .post("/v1/mods", &wire::ModsRequest { mod_ids: chunk })
                .await?;
            found.extend(answer.data.into_iter().map(|project| CurseForgeProject {
                id: project.id,
                name: project.name,
                class_id: project.class_id,
            }));
        }
        Ok(found)
    }
}

/// Reads a pack's `manifest.json`.
pub fn parse_manifest(bytes: &[u8]) -> Result<RawManifest, PackFileError> {
    let manifest: wire::Manifest = serde_json::from_slice(bytes)
        .map_err(|error| PackFileError::InvalidArchive(format!("manifest.json: {error}")))?;
    Ok(RawManifest {
        manifest_type: manifest.manifest_type,
        manifest_version: manifest.manifest_version,
        name: manifest.name,
        version: manifest.version,
        game_version: manifest.minecraft.version,
        loaders: manifest
            .minecraft
            .mod_loaders
            .into_iter()
            .map(|loader| RawLoader {
                id: loader.id,
                primary: loader.primary,
            })
            .collect(),
        files: manifest
            .files
            .into_iter()
            .map(|file| RawFileRef {
                project_id: file.project_id,
                file_id: file.file_id,
                required: file.required,
            })
            .collect(),
        overrides: manifest.overrides,
    })
}

fn summary(project: wire::Mod) -> ModpackSummary {
    let loaders = pack_loaders(
        project
            .latest_files_indexes
            .iter()
            .filter_map(|index| index.mod_loader)
            .filter_map(loader_kind),
    );
    ModpackSummary {
        loaders,
        source: ModpackSourceKind::CurseForge,
        project: project.id.to_string(),
        title: project.name,
        description: project.summary.unwrap_or_default(),
        icon_url: project.logo.and_then(|logo| logo.thumbnail_url),
        // CurseForge reports the count as a JSON number that may be a float.
        downloads: project
            .download_count
            .filter(|count| count.is_finite() && *count >= 0.0)
            .map(|count| count as u64),
    }
}

fn sha1(file: &wire::File) -> Option<String> {
    file.hashes
        .iter()
        .find(|hash| hash.algo == wire::SHA1_ALGO)
        .map(|hash| hash.value.to_ascii_lowercase())
}

fn file(file: wire::File) -> CurseForgeFile {
    CurseForgeFile {
        sha1: sha1(&file),
        id: file.id,
        project_id: file.mod_id,
        file_name: file.file_name,
        length: file.file_length,
        download_url: file.download_url,
    }
}

fn version(file: wire::File) -> ModpackVersion {
    let pack_file = match (&file.download_url, file.file_length, sha1(&file)) {
        (Some(url), Some(size), Some(sha1)) if is_allowed_download(url) => {
            Checksum::parse(HashAlgorithm::Sha1, &sha1)
                .ok()
                .map(|checksum| ModpackFile {
                    url: url.clone(),
                    filename: file.file_name.clone(),
                    size,
                    checksum,
                })
        }
        _ => None,
    };
    let loaders = file
        .game_versions
        .iter()
        .filter_map(|name| match name.to_ascii_lowercase().as_str() {
            "fabric" => Some(LoaderKind::Fabric),
            "quilt" => Some(LoaderKind::Quilt),
            "forge" => Some(LoaderKind::Forge),
            "neoforge" => Some(LoaderKind::NeoForge),
            _ => None,
        })
        .collect();
    // CurseForge mixes loader names into `gameVersions`; versions start with a digit.
    let game_versions = file
        .game_versions
        .iter()
        .filter(|name| name.starts_with(|c: char| c.is_ascii_digit()))
        .filter_map(|name| GameVersion::parse(name).ok())
        .collect();
    let name = file.display_name.unwrap_or_else(|| file.file_name.clone());
    ModpackVersion {
        id: file.id.to_string(),
        version_number: name.clone(),
        name,
        game_versions,
        loaders,
        published: file.file_date,
        pack_file,
    }
}

/// The loader behind a CurseForge `modLoaderType` value.
fn loader_kind(code: u8) -> Option<LoaderKind> {
    [
        LoaderKind::Forge,
        LoaderKind::Fabric,
        LoaderKind::Quilt,
        LoaderKind::NeoForge,
    ]
    .into_iter()
    .find(|kind| loader_type(*kind) == Some(code))
}

/// CurseForge `modLoaderType` values.
fn loader_type(kind: LoaderKind) -> Option<u8> {
    match kind {
        LoaderKind::Vanilla => None,
        LoaderKind::Forge => Some(1),
        LoaderKind::Fabric => Some(4),
        LoaderKind::Quilt => Some(5),
        LoaderKind::NeoForge => Some(6),
    }
}

/// CurseForge project ids are decimal numbers; anything else is refused
/// before it becomes part of a URL.
fn checked_id(project: &str) -> Result<u64, ModpackSourceError> {
    project
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0 && project.len() <= 12 && project.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| ModpackSourceError::NotFound(project.to_owned()))
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
        // CurseForge answers 401/403 when the key is missing, wrong or revoked.
        HttpError::Status {
            status: 401 | 403, ..
        } => ModpackSourceError::Network("CurseForge refused the launcher's API key".into()),
        other @ (HttpError::Status { .. }
        | HttpError::Network { .. }
        | HttpError::TooLarge { .. }) => ModpackSourceError::Network(other.to_string()),
    }
}
