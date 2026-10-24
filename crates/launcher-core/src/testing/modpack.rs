//! Fakes for the modpack file ports: `.mrpack` archives, the game folder and
//! the installed-files manifest.
//!
//! `FakePackFiles` works on real paths (tests point it at a temporary folder)
//! because `FakeDownloader` writes real files too.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha1::{Digest, Sha1};
use sha2::Sha512;

use crate::domain::instance::InstanceId;
use crate::domain::integrity::{Checksum, HashAlgorithm, curseforge_fingerprint};
use crate::domain::launch::RelativePath;
use crate::domain::modpack::mrpack::MrpackIndex;
use crate::domain::modpack::packwiz::{RawMetafile, RawPackIndex, RawPackToml};
use crate::domain::modpack::sync::{LocalFile, PackManifest};
use crate::domain::modpack::{
    MrpackArchive, PackFileError, PackFileLocation, PackFiles, PackLocation, PackManifestStore,
    PackwizSource, StagedFile,
};

fn io(path: &Path, error: std::io::Error) -> PackFileError {
    PackFileError::Io {
        path: path.display().to_string(),
        reason: error.to_string(),
    }
}

/// Lowercase hex sha1 of `bytes`.
pub fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

/// Lowercase hex sha512 of `bytes`.
pub fn sha512_hex(bytes: &[u8]) -> String {
    hex::encode(Sha512::digest(bytes))
}

/// A recorded `write`: archive path, index and override sources.
pub type WrittenPack = (PathBuf, MrpackIndex, Vec<(RelativePath, PathBuf)>);

/// Serves fixed indexes and override files per archive path; records writes.
#[derive(Debug, Default)]
pub struct FakeMrpackArchive {
    pub indexes: HashMap<PathBuf, MrpackIndex>,
    /// Override files (path in the game folder, content) per archive.
    pub overrides: HashMap<PathBuf, Vec<(RelativePath, Vec<u8>)>>,
    written: Mutex<Vec<WrittenPack>>,
}

impl FakeMrpackArchive {
    pub fn written(&self) -> Vec<WrittenPack> {
        self.written.lock().unwrap().clone()
    }
}

impl MrpackArchive for FakeMrpackArchive {
    async fn read_index(&self, archive: &Path) -> Result<MrpackIndex, PackFileError> {
        self.indexes
            .get(archive)
            .cloned()
            .ok_or_else(|| PackFileError::InvalidArchive(archive.display().to_string()))
    }

    async fn extract_overrides(
        &self,
        archive: &Path,
        staging: &Path,
    ) -> Result<Vec<StagedFile>, PackFileError> {
        let mut staged = Vec::new();
        for (path, bytes) in self.overrides.get(archive).into_iter().flatten() {
            let target = path.under(staging);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
            }
            std::fs::write(&target, bytes).map_err(|e| io(&target, e))?;
            staged.push(StagedFile {
                path: path.clone(),
                sha1: sha1_hex(bytes),
                staged: target,
            });
        }
        Ok(staged)
    }

    async fn write(
        &self,
        archive: &Path,
        index: &MrpackIndex,
        overrides: &[(RelativePath, PathBuf)],
    ) -> Result<(), PackFileError> {
        self.written.lock().unwrap().push((
            archive.to_path_buf(),
            index.clone(),
            overrides.to_vec(),
        ));
        Ok(())
    }
}

/// The game-folder port over the real file system; records removals.
#[derive(Debug, Default)]
pub struct FakePackFiles {
    removed: Mutex<Vec<PathBuf>>,
    /// When set, `place` succeeds this many more times and then fails.
    places_before_failure: Mutex<Option<usize>>,
}

impl FakePackFiles {
    pub fn removed(&self) -> Vec<PathBuf> {
        self.removed.lock().unwrap().clone()
    }

    /// Lets `successful_places` moves through, then fails every later one.
    pub fn fail_places_after(&self, successful_places: usize) {
        *self.places_before_failure.lock().unwrap() = Some(successful_places);
    }
}

impl PackFiles for FakePackFiles {
    async fn sha1(&self, path: &Path) -> Result<Option<String>, PackFileError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(sha1_hex(&bytes))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io(path, error)),
        }
    }

    async fn digest(
        &self,
        path: &Path,
        algorithm: HashAlgorithm,
    ) -> Result<Option<String>, PackFileError> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io(path, error)),
        };
        Ok(Some(match algorithm {
            HashAlgorithm::Sha1 => sha1_hex(&bytes),
            HashAlgorithm::Sha256 => hex::encode(sha2::Sha256::digest(&bytes)),
            HashAlgorithm::Sha512 => sha512_hex(&bytes),
            HashAlgorithm::Murmur2 => curseforge_fingerprint(&bytes).to_string(),
        }))
    }

    async fn list_dir(&self, dir: &Path) -> Result<Vec<PathBuf>, PackFileError> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(Vec::new());
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .map(|entry| entry.path())
            .collect();
        files.sort();
        Ok(files)
    }

    async fn place(&self, from: &Path, to: &Path) -> Result<(), PackFileError> {
        if let Some(remaining) = self.places_before_failure.lock().unwrap().as_mut() {
            if *remaining == 0 {
                return Err(io(to, std::io::Error::other("no space left on device")));
            }
            *remaining -= 1;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        std::fs::copy(from, to).map_err(|e| io(to, e))?;
        Ok(())
    }

    async fn remove_file(&self, path: &Path) -> Result<(), PackFileError> {
        self.removed.lock().unwrap().push(path.to_path_buf());
        match std::fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(io(path, error)),
            _ => Ok(()),
        }
    }

    async fn scan(&self, root: &Path, entries: &[&str]) -> Result<Vec<LocalFile>, PackFileError> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            if dir.is_file() {
                out.push(dir.to_path_buf());
                return;
            }
            let Ok(read) = std::fs::read_dir(dir) else {
                return;
            };
            let mut children: Vec<PathBuf> = read.flatten().map(|entry| entry.path()).collect();
            children.sort();
            for child in children {
                walk(&child, out);
            }
        }
        let mut found = Vec::new();
        for entry in entries {
            walk(&root.join(entry), &mut found);
        }
        let mut files = Vec::new();
        for path in found {
            let bytes = std::fs::read(&path).map_err(|e| io(&path, e))?;
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            files.push(LocalFile {
                path: RelativePath::parse(&relative).unwrap(),
                sha1: Checksum::parse(HashAlgorithm::Sha1, &sha1_hex(&bytes)).unwrap(),
                sha512: Checksum::parse(HashAlgorithm::Sha512, &sha512_hex(&bytes)).unwrap(),
                size: bytes.len() as u64,
            });
        }
        Ok(files)
    }
}

/// Keeps manifests in memory.
#[derive(Debug, Default)]
pub struct InMemoryPackManifestStore {
    pub manifests: Mutex<HashMap<InstanceId, PackManifest>>,
}

impl PackManifestStore for InMemoryPackManifestStore {
    async fn load(&self, id: &InstanceId) -> Result<PackManifest, PackFileError> {
        Ok(self
            .manifests
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default())
    }

    async fn save(&self, id: &InstanceId, manifest: &PackManifest) -> Result<(), PackFileError> {
        self.manifests
            .lock()
            .unwrap()
            .insert(id.clone(), manifest.clone());
        Ok(())
    }
}

/// Serves parsed pack files from memory by path relative to the pack folder,
/// and records which checksums the caller asked to verify. `locate` points
/// at `url_base` (or `/pack` when `None`) so tests can assert the requests.
#[derive(Debug)]
pub struct FakePackwizSource {
    pub pack: RawPackToml,
    pub index: RawPackIndex,
    pub metafiles: HashMap<String, RawMetafile>,
    /// `https://example.com/pack/` style prefix; `None` locates local paths under `/pack`.
    pub url_base: Option<String>,
    verified: Mutex<Vec<(String, Checksum)>>,
}

impl FakePackwizSource {
    pub fn new(pack: RawPackToml, index: RawPackIndex) -> Self {
        Self {
            pack,
            index,
            metafiles: HashMap::new(),
            url_base: Some("https://example.com/pack/".into()),
            verified: Mutex::default(),
        }
    }

    /// Every (path, expected checksum) the caller passed to a verified read.
    pub fn verified(&self) -> Vec<(String, Checksum)> {
        self.verified.lock().unwrap().clone()
    }
}

impl PackwizSource for FakePackwizSource {
    async fn read_pack(&self, _pack: &PackLocation) -> Result<RawPackToml, PackFileError> {
        Ok(self.pack.clone())
    }

    async fn read_index(
        &self,
        _pack: &PackLocation,
        relative: &RelativePath,
        expected: &Checksum,
    ) -> Result<RawPackIndex, PackFileError> {
        self.verified
            .lock()
            .unwrap()
            .push((relative.to_string(), expected.clone()));
        Ok(self.index.clone())
    }

    async fn read_metafile(
        &self,
        _pack: &PackLocation,
        relative: &RelativePath,
        expected: &Checksum,
    ) -> Result<RawMetafile, PackFileError> {
        self.verified
            .lock()
            .unwrap()
            .push((relative.to_string(), expected.clone()));
        self.metafiles
            .get(relative.as_str())
            .cloned()
            .ok_or_else(|| PackFileError::Source(format!("{relative} was not found")))
    }

    fn locate(
        &self,
        _pack: &PackLocation,
        relative: &RelativePath,
    ) -> Result<PackFileLocation, PackFileError> {
        Ok(match &self.url_base {
            Some(base) => PackFileLocation::Url(format!("{base}{}", relative.as_str())),
            None => PackFileLocation::Path(relative.under(Path::new("/pack"))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pack_files_hash_place_remove_and_scan_real_paths() {
        let dir = tempfile::tempdir().unwrap();
        let files = FakePackFiles::default();
        let source = dir.path().join("staged.txt");
        std::fs::write(&source, b"abc").unwrap();
        let target = dir.path().join("game/mods/a.jar");
        assert_eq!(files.sha1(&target).await.unwrap(), None);
        files.place(&source, &target).await.unwrap();
        assert_eq!(files.sha1(&target).await.unwrap(), Some(sha1_hex(b"abc")));
        let scanned = files
            .scan(&dir.path().join("game"), &["mods", "config"])
            .await
            .unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].path.as_str(), "mods/a.jar");
        files.remove_file(&target).await.unwrap();
        files.remove_file(&target).await.unwrap();
        assert!(!target.exists());
    }
}
