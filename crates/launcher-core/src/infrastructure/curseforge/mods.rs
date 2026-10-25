//! Individual mods from CurseForge (class 6): search for a target, a
//! project's fitting files with their dependencies, single files, and
//! project batches. Ids are namespaced as `curseforge:<number>` (see
//! [`mod_catalog`](crate::domain::mods::mod_catalog)).
//!
//! A file whose author keeps it off third-party launchers has no download
//! URL: it becomes a version without a jar that points to the project page.

use std::collections::HashMap;

use super::{BATCH, CurseForgeSource, FILES_PAGE, MINECRAFT_GAME_ID, loader_type, sha1, wire};
use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::RelativePath;
use crate::domain::loader::LoaderKind;
use crate::domain::modpack::ModpackSourceError;
use crate::domain::modpack::curseforge::is_allowed_download;
use crate::domain::mods::{
    DependencyKind, ModDependency, ModFile, ModPage, ModQuery, ModSource, ModSummary, ModTarget,
    ModVersion, ReleaseChannel, curseforge_mod_id, curseforge_number,
};
use crate::domain::ports::HttpClient;
use crate::infrastructure::modrinth::encode;

const MOD_CLASS_ID: u64 = 6;
const JAR_EXTENSION: &str = ".jar";

impl<H: HttpClient> ModSource for CurseForgeSource<H> {
    async fn search(&self, query: &ModQuery) -> Result<ModPage, ModpackSourceError> {
        let query = query.clone().normalized();
        // Nothing runs on Vanilla; ask nothing.
        let Some(loader) = loader_type(query.target.loader) else {
            return Ok(ModPage {
                items: Vec::new(),
                total: Some(0),
            });
        };
        let index = u64::from(query.page) * u64::from(query.page_size);
        // Sorted by popularity, like the CurseForge site. The loader filter
        // takes one loader, so a Quilt instance finds mods tagged Quilt.
        let path = format!(
            "/v1/mods/search?gameId={MINECRAFT_GAME_ID}&classId={MOD_CLASS_ID}\
             &searchFilter={}&sortField=2&sortOrder=desc&index={index}&pageSize={}\
             &gameVersion={}&modLoaderType={loader}",
            encode(&query.text),
            query.page_size,
            encode(query.target.game_version.as_str()),
        );
        let answer: wire::SearchResponse = self.get(&path, "search").await?;
        Ok(ModPage {
            items: answer.data.into_iter().map(summary).collect(),
            total: answer.pagination.and_then(|page| page.total_count),
        })
    }

    async fn versions(
        &self,
        project: &str,
        target: &ModTarget,
    ) -> Result<Vec<ModVersion>, ModpackSourceError> {
        let id = checked(project)?;
        if target.accepted_loaders().is_empty() {
            return Ok(Vec::new());
        }
        // Filtered by game version here; loaders are checked below, since a
        // Quilt instance also runs Fabric files.
        let path = format!(
            "/v1/mods/{id}/files?gameVersion={}&pageSize={FILES_PAGE}",
            encode(target.game_version.as_str()),
        );
        let answer: wire::Data<Vec<wire::File>> = self.get(&path, project).await?;
        let mut versions: Vec<ModVersion> = answer
            .data
            .into_iter()
            .filter(|file| file.mod_id == id && file.is_server_pack != Some(true))
            .map(mod_version)
            .filter(|version| target.accepts(version))
            .collect();
        // Newest first, whatever order CurseForge used.
        versions.sort_by(|a, b| b.published.cmp(&a.published));
        Ok(versions)
    }

    async fn version(&self, id: &str) -> Result<ModVersion, ModpackSourceError> {
        let file_id = checked(id)?;
        let answer: wire::Data<Vec<wire::File>> = self
            .post(
                "/v1/mods/files",
                &wire::FilesRequest {
                    file_ids: &[file_id],
                },
            )
            .await?;
        answer
            .data
            .into_iter()
            .find(|file| file.id == file_id)
            .map(mod_version)
            .ok_or_else(|| ModpackSourceError::NotFound(id.to_owned()))
    }

    async fn projects(&self, ids: &[String]) -> Result<Vec<ModSummary>, ModpackSourceError> {
        let numbers: Vec<u64> = ids.iter().filter_map(|id| curseforge_number(id)).collect();
        let mut projects = Vec::new();
        for chunk in numbers.chunks(BATCH) {
            let answer: wire::Data<Vec<wire::Mod>> = self
                .post("/v1/mods", &wire::ModsRequest { mod_ids: chunk })
                .await?;
            projects.extend(
                answer
                    .data
                    .into_iter()
                    .filter(|project| {
                        chunk.contains(&project.id) && project.class_id == Some(MOD_CLASS_ID)
                    })
                    .map(summary),
            );
        }
        Ok(projects)
    }

    /// Loose jars are identified by Modrinth only: CurseForge knows files by
    /// fingerprint, not sha512.
    async fn identify(
        &self,
        _sha512: &[String],
    ) -> Result<HashMap<String, ModVersion>, ModpackSourceError> {
        Ok(HashMap::new())
    }
}

/// The number behind a `curseforge:` id; anything else is refused before it
/// becomes part of a URL.
fn checked(id: &str) -> Result<u64, ModpackSourceError> {
    curseforge_number(id).ok_or_else(|| ModpackSourceError::NotFound(id.to_owned()))
}

fn summary(project: wire::Mod) -> ModSummary {
    ModSummary {
        project: curseforge_mod_id(project.id),
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

fn mod_version(file: wire::File) -> ModVersion {
    let jar = mod_file(&file);
    // No URL at all: the author lets only CurseForge's site hand it out.
    let manual_page = file
        .download_url
        .is_none()
        .then(|| format!("https://www.curseforge.com/projects/{}", file.mod_id));
    // CurseForge mixes loader names into `gameVersions`; versions start with a digit.
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
    let game_versions = file
        .game_versions
        .iter()
        .filter(|name| name.starts_with(|c: char| c.is_ascii_digit()))
        .filter_map(|name| GameVersion::parse(name).ok())
        .collect();
    let name = file.display_name.unwrap_or_else(|| file.file_name.clone());
    ModVersion {
        id: curseforge_mod_id(file.id),
        project: curseforge_mod_id(file.mod_id),
        version_number: name.clone(),
        name,
        channel: match file.release_type {
            Some(2) => ReleaseChannel::Beta,
            Some(3) => ReleaseChannel::Alpha,
            _ => ReleaseChannel::Release,
        },
        game_versions,
        loaders,
        published: file.file_date,
        file: jar,
        manual_page,
        dependencies: file.dependencies.iter().filter_map(dependency).collect(),
    }
}

/// CurseForge's `FileRelationType`; tools (4) are not part of the game.
fn dependency(raw: &wire::FileDependency) -> Option<ModDependency> {
    let kind = match raw.relation_type {
        1 | 6 => DependencyKind::Embedded,
        2 => DependencyKind::Optional,
        3 => DependencyKind::Required,
        5 => DependencyKind::Incompatible,
        _ => return None,
    };
    (raw.mod_id > 0).then(|| ModDependency {
        project: Some(curseforge_mod_id(raw.mod_id)),
        version: None,
        kind,
    })
}

/// The file as a jar to install: `None` without an allowed download URL, a
/// length and a valid sha1, or when its name is not a single safe `.jar`.
fn mod_file(file: &wire::File) -> Option<ModFile> {
    let url = file
        .download_url
        .as_ref()
        .filter(|url| is_allowed_download(url))?;
    let checksum = Checksum::parse(HashAlgorithm::Sha1, &sha1(file)?).ok()?;
    let is_jar = file.file_name.to_ascii_lowercase().ends_with(JAR_EXTENSION);
    if !is_jar || !RelativePath::is_file_name(&file.file_name) {
        return None;
    }
    Some(ModFile {
        url: url.clone(),
        filename: file.file_name.clone(),
        size: file.file_length?,
        checksum,
    })
}

#[cfg(test)]
mod tests;
