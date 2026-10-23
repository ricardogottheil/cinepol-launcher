//! Fakes for the CurseForge ports: the catalog with its file and project
//! lookups, and pack zips.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::modpack::sha1_hex;
use crate::domain::launch::RelativePath;
use crate::domain::modpack::curseforge::{CurseForgeFile, CurseForgeProject, RawManifest};
use crate::domain::modpack::{
    CurseForgeArchive, CurseForgeLookup, ModpackPage, ModpackQuery, ModpackSource,
    ModpackSourceError, ModpackSourceKind, ModpackSummary, ModpackVersion, PackFileError,
    StagedFile,
};

/// A CurseForge catalog with per-project versions and known files and
/// projects; records which ids each lookup asked for.
#[derive(Debug, Default)]
pub struct FakeCurseForge {
    pub versions: HashMap<String, Vec<ModpackVersion>>,
    pub files: Vec<CurseForgeFile>,
    pub projects: Vec<CurseForgeProject>,
    /// Fails every call with this error.
    pub failure: Option<ModpackSourceError>,
    pub(crate) file_lookups: Mutex<Vec<Vec<u64>>>,
    pub(crate) project_lookups: Mutex<Vec<Vec<u64>>>,
}

impl FakeCurseForge {
    pub fn file_lookups(&self) -> Vec<Vec<u64>> {
        self.file_lookups.lock().unwrap().clone()
    }

    pub fn project_lookups(&self) -> Vec<Vec<u64>> {
        self.project_lookups.lock().unwrap().clone()
    }

    fn fail(&self) -> Result<(), ModpackSourceError> {
        match &self.failure {
            Some(ModpackSourceError::NotConfigured(kind)) => {
                Err(ModpackSourceError::NotConfigured(*kind))
            }
            Some(other) => Err(ModpackSourceError::Network(other.to_string())),
            None => Ok(()),
        }
    }
}

impl ModpackSource for FakeCurseForge {
    fn kind(&self) -> ModpackSourceKind {
        ModpackSourceKind::CurseForge
    }

    async fn search(&self, _query: &ModpackQuery) -> Result<ModpackPage, ModpackSourceError> {
        self.fail()?;
        Ok(ModpackPage {
            items: vec![],
            total: Some(0),
        })
    }

    async fn project(&self, project: &str) -> Result<ModpackSummary, ModpackSourceError> {
        self.fail()?;
        Err(ModpackSourceError::NotFound(project.to_owned()))
    }

    async fn versions(&self, project: &str) -> Result<Vec<ModpackVersion>, ModpackSourceError> {
        self.fail()?;
        self.versions
            .get(project)
            .cloned()
            .ok_or_else(|| ModpackSourceError::NotFound(project.to_owned()))
    }

    async fn known_files(
        &self,
        _sha512: &[String],
    ) -> Result<HashMap<String, Vec<String>>, ModpackSourceError> {
        Ok(HashMap::new())
    }
}

impl CurseForgeLookup for FakeCurseForge {
    async fn files(&self, ids: &[u64]) -> Result<Vec<CurseForgeFile>, ModpackSourceError> {
        self.fail()?;
        self.file_lookups.lock().unwrap().push(ids.to_vec());
        Ok(self
            .files
            .iter()
            .filter(|file| ids.contains(&file.id))
            .cloned()
            .collect())
    }

    async fn projects(&self, ids: &[u64]) -> Result<Vec<CurseForgeProject>, ModpackSourceError> {
        self.fail()?;
        self.project_lookups.lock().unwrap().push(ids.to_vec());
        Ok(self
            .projects
            .iter()
            .filter(|project| ids.contains(&project.id))
            .cloned()
            .collect())
    }
}

/// A catalog that knows no files: CurseForge-mode files stay manual.
pub fn no_curseforge() -> &'static FakeCurseForge {
    static NONE: std::sync::LazyLock<FakeCurseForge> =
        std::sync::LazyLock::new(FakeCurseForge::default);
    &NONE
}

/// Serves a manifest and override files per archive path; records the
/// overrides folder each extraction asked for.
#[derive(Debug, Default)]
pub struct FakeCurseForgeArchive {
    pub manifests: HashMap<PathBuf, RawManifest>,
    /// Files under the overrides folder (path in the game folder, content).
    pub overrides: HashMap<PathBuf, Vec<(RelativePath, Vec<u8>)>>,
    pub(crate) folders: Mutex<Vec<String>>,
}

impl FakeCurseForgeArchive {
    pub fn folders(&self) -> Vec<String> {
        self.folders.lock().unwrap().clone()
    }
}

impl CurseForgeArchive for FakeCurseForgeArchive {
    async fn read_manifest(&self, archive: &Path) -> Result<RawManifest, PackFileError> {
        self.manifests
            .get(archive)
            .cloned()
            .ok_or_else(|| PackFileError::InvalidArchive(archive.display().to_string()))
    }

    async fn extract_overrides(
        &self,
        archive: &Path,
        folder: &str,
        staging: &Path,
    ) -> Result<Vec<StagedFile>, PackFileError> {
        self.folders.lock().unwrap().push(folder.to_owned());
        let mut staged = Vec::new();
        for (path, bytes) in self.overrides.get(archive).into_iter().flatten() {
            let target = path.under(staging);
            let io = |error: std::io::Error| PackFileError::Io {
                path: target.display().to_string(),
                reason: error.to_string(),
            };
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(io)?;
            }
            std::fs::write(&target, bytes).map_err(io)?;
            staged.push(StagedFile {
                path: path.clone(),
                sha1: sha1_hex(bytes),
                staged: target,
            });
        }
        Ok(staged)
    }
}
