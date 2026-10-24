//! Individual mods from Modrinth: search for a target, its fitting versions
//! with their dependencies, single versions, and project batches.

use super::{checked_id, encode, publication_key, wire};
use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::RelativePath;
use crate::domain::loader::LoaderKind;
use crate::domain::modpack::ModpackSourceError;
use crate::domain::modpack::mrpack::is_allowed_download;
use crate::domain::mods::{
    DependencyKind, ModDependency, ModFile, ModPage, ModQuery, ModSource, ModSummary, ModTarget,
    ModVersion, ReleaseChannel,
};
use crate::domain::ports::HttpClient;

use super::ModrinthSource;

const MOD_TYPE: &str = "mod";
const JAR_EXTENSION: &str = ".jar";
/// Project ids per `GET /projects` request, keeping the URL short.
const PROJECTS_BATCH: usize = 100;

impl<H: HttpClient> ModSource for ModrinthSource<H> {
    async fn search(&self, query: &ModQuery) -> Result<ModPage, ModpackSourceError> {
        let query = query.clone().normalized();
        let loaders = query.target.accepted_loaders();
        // Nothing runs on Vanilla; ask nothing.
        if loaders.is_empty() {
            return Ok(ModPage {
                items: Vec::new(),
                total: Some(0),
            });
        }
        let facets = vec![
            vec![format!("project_type:{MOD_TYPE}")],
            vec![format!("versions:{}", query.target.game_version.as_str())],
            loaders
                .iter()
                .map(|loader| format!("categories:{}", loader.id()))
                .collect(),
            // The launcher runs the client: server-only mods are left out.
            vec![
                "client_side:required".to_owned(),
                "client_side:optional".to_owned(),
            ],
        ];
        let facets = to_json(&facets)?;
        let offset = u64::from(query.page) * u64::from(query.page_size);
        let path = format!(
            "/search?query={}&facets={}&index=relevance&offset={offset}&limit={}",
            encode(&query.text),
            encode(&facets),
            query.page_size,
        );
        let answer: wire::SearchResponse = self.get(&path, "search").await?;
        Ok(ModPage {
            items: answer
                .hits
                .into_iter()
                .map(|hit| ModSummary {
                    project: hit.project_id,
                    title: hit.title,
                    description: hit.description.unwrap_or_default(),
                    icon_url: hit.icon_url,
                    downloads: hit.downloads,
                })
                .collect(),
            total: answer.total_hits,
        })
    }

    async fn versions(
        &self,
        project: &str,
        target: &ModTarget,
    ) -> Result<Vec<ModVersion>, ModpackSourceError> {
        let id = checked_id(project)?;
        let loaders: Vec<&str> = target
            .accepted_loaders()
            .into_iter()
            .map(LoaderKind::id)
            .collect();
        if loaders.is_empty() {
            return Ok(Vec::new());
        }
        let path = format!(
            "/project/{id}/version?loaders={}&game_versions={}",
            encode(&to_json(&loaders)?),
            encode(&to_json(&[target.game_version.as_str()])?),
        );
        let answer: Vec<wire::Version> = self.get(&path, project).await?;
        let mut versions: Vec<(Option<String>, ModVersion)> = answer
            .into_iter()
            .map(|version| {
                let key = version.date_published.as_deref().and_then(publication_key);
                (key, mod_version(version))
            })
            // The API filters already; anything else is not trusted.
            .filter(|(_, version)| target.accepts(version))
            .collect();
        // Newest first; undated versions last, in the order the API gave them.
        versions.sort_by(|(a, _), (b, _)| b.cmp(a));
        Ok(versions.into_iter().map(|(_, version)| version).collect())
    }

    async fn version(&self, id: &str) -> Result<ModVersion, ModpackSourceError> {
        let checked = checked_id(id)?;
        let answer: wire::Version = self.get(&format!("/version/{checked}"), id).await?;
        Ok(mod_version(answer))
    }

    async fn projects(&self, ids: &[String]) -> Result<Vec<ModSummary>, ModpackSourceError> {
        let ids: Vec<&str> = ids.iter().filter_map(|id| checked_id(id).ok()).collect();
        let mut projects = Vec::new();
        for batch in ids.chunks(PROJECTS_BATCH) {
            let path = format!("/projects?ids={}", encode(&to_json(&batch)?));
            let answer: Vec<wire::Project> = self.get(&path, "projects").await?;
            projects.extend(
                answer
                    .into_iter()
                    .filter(|project| project.project_type == MOD_TYPE)
                    .map(|project| ModSummary {
                        project: project.id,
                        title: project.title,
                        description: project.description.unwrap_or_default(),
                        icon_url: project.icon_url,
                        downloads: project.downloads,
                    }),
            );
        }
        Ok(projects)
    }
}

fn mod_version(version: wire::Version) -> ModVersion {
    ModVersion {
        file: mod_file(&version.files),
        name: version.name.unwrap_or_else(|| version.id.clone()),
        version_number: version.version_number.unwrap_or_else(|| version.id.clone()),
        channel: match version.version_type.as_deref() {
            Some("beta") => ReleaseChannel::Beta,
            Some("alpha") => ReleaseChannel::Alpha,
            _ => ReleaseChannel::Release,
        },
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
        dependencies: version
            .dependencies
            .into_iter()
            .filter_map(dependency)
            .collect(),
        id: version.id,
        project: version.project_id,
    }
}

/// A dependency with a known kind that names a valid project or version.
fn dependency(raw: wire::Dependency) -> Option<ModDependency> {
    let kind = match raw.dependency_type.as_deref().unwrap_or_default() {
        "required" => DependencyKind::Required,
        "optional" => DependencyKind::Optional,
        "incompatible" => DependencyKind::Incompatible,
        "embedded" => DependencyKind::Embedded,
        _ => return None,
    };
    let valid = |id: Option<String>| id.filter(|id| checked_id(id).is_ok());
    let project = valid(raw.project_id);
    let version = valid(raw.version_id);
    (project.is_some() || version.is_some()).then_some(ModDependency {
        project,
        version,
        kind,
    })
}

/// The primary `.jar` file, else the first one; `None` when it lacks a valid
/// sha512, is not on an allowed download host, or its name is not a single
/// safe file name.
fn mod_file(files: &[wire::VersionFile]) -> Option<ModFile> {
    let is_jar =
        |file: &&wire::VersionFile| file.filename.to_ascii_lowercase().ends_with(JAR_EXTENSION);
    let file = files
        .iter()
        .filter(is_jar)
        .find(|file| file.primary)
        .or_else(|| files.iter().find(is_jar))?;
    let sha512 = Checksum::parse(HashAlgorithm::Sha512, file.hashes.get("sha512")?).ok()?;
    let sha1 = file
        .hashes
        .get("sha1")
        .and_then(|hash| Checksum::parse(HashAlgorithm::Sha1, hash).ok());
    let safe_name = RelativePath::parse(&file.filename).is_ok() && !file.filename.contains('/');
    if !safe_name || !is_allowed_download(&file.url) {
        return None;
    }
    Some(ModFile {
        url: file.url.clone(),
        filename: file.filename.clone(),
        size: file.size,
        sha512,
        sha1,
    })
}

fn to_json<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, ModpackSourceError> {
    serde_json::to_string(value)
        .map_err(|error| ModpackSourceError::InvalidResponse(error.to_string()))
}

#[cfg(test)]
mod tests;
