//! CurseForge modpacks: the `manifest.json` inside a pack zip, and turning
//! its file references into downloads or manual downloads.
//!
//! The manifest lists project and file ids only. Where a file goes depends on
//! its project's class (mods, resource packs, shader packs); whether the
//! launcher may download it depends on the file having a download URL (its
//! author may forbid third-party distribution).

use std::collections::{BTreeSet, HashMap};

use thiserror::Error;

use super::sync::{EntrySource, ManualFile, PackEntry};
use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::RelativePath;
use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};

/// The only manifest type and version this launcher reads.
pub const MANIFEST_TYPE: &str = "minecraftModpack";
pub const MANIFEST_VERSION: u32 = 1;

/// Hosts CurseForge serves files from.
pub const ALLOWED_DOWNLOAD_HOSTS: [&str; 2] = ["edge.forgecdn.net", "mediafilez.forgecdn.net"];

/// Project classes this launcher installs, with the game folder they go into.
pub const CLASS_FOLDERS: [(u64, &str); 3] =
    [(6, "mods"), (12, "resourcepacks"), (6552, "shaderpacks")];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CurseForgeError {
    #[error("unsupported manifest {kind:?} version {version}")]
    UnsupportedManifest { kind: String, version: u32 },
    #[error("invalid Minecraft version {0:?}")]
    InvalidGameVersion(String),
    #[error("unsupported mod loader {0:?}")]
    UnsupportedLoader(String),
    #[error("the pack declares more than one primary mod loader")]
    SeveralLoaders,
    #[error("unsafe overrides folder {0:?}")]
    UnsafeOverrides(String),
    #[error("project {project} file {file} is listed twice")]
    DuplicateFile { project: u64, file: u64 },
    #[error("CurseForge did not return file {0}")]
    MissingFile(u64),
    #[error("file {file} does not belong to project {project}")]
    WrongProject { project: u64, file: u64 },
    #[error("CurseForge did not return project {0}")]
    MissingProject(u64),
    #[error("project {project} is a {class:?} project, which cannot be installed")]
    UnsupportedClass { project: u64, class: Option<u64> },
    #[error("unsafe file name {0:?}")]
    UnsafeFileName(String),
    #[error("{0} does not declare a valid sha1")]
    UnverifiedFile(String),
    #[error("download host not allowed: {0}")]
    DisallowedDownload(String),
    #[error("{0} would be installed twice")]
    DuplicatePath(String),
}

/// `manifest.json` as written in the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawManifest {
    pub manifest_type: String,
    pub manifest_version: u32,
    pub name: String,
    pub version: Option<String>,
    pub game_version: String,
    pub loaders: Vec<RawLoader>,
    pub files: Vec<RawFileRef>,
    /// The overrides folder inside the zip; `overrides` when absent.
    pub overrides: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLoader {
    /// `fabric-0.19.5`, `forge-47.2.0`, `neoforge-21.1.1`, `quilt-0.26.0`.
    pub id: String,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawFileRef {
    pub project_id: u64,
    pub file_id: u64,
    pub required: bool,
}

/// A file the pack installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileRef {
    pub project_id: u64,
    pub file_id: u64,
}

/// A checked manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurseForgePack {
    pub name: String,
    pub version: Option<String>,
    pub game_version: GameVersion,
    pub loader: LoaderSpec,
    /// Required files only, in manifest order.
    pub files: Vec<FileRef>,
    /// One safe path segment.
    pub overrides: String,
}

impl CurseForgePack {
    pub fn validate(raw: RawManifest) -> Result<Self, CurseForgeError> {
        if raw.manifest_type != MANIFEST_TYPE || raw.manifest_version != MANIFEST_VERSION {
            return Err(CurseForgeError::UnsupportedManifest {
                kind: raw.manifest_type,
                version: raw.manifest_version,
            });
        }
        let game_version = GameVersion::parse(&raw.game_version)
            .map_err(|_| CurseForgeError::InvalidGameVersion(raw.game_version.clone()))?;
        let loader = loader(&raw.loaders)?;
        let overrides = raw.overrides.unwrap_or_else(|| "overrides".to_owned());
        if !is_safe_segment(&overrides) {
            return Err(CurseForgeError::UnsafeOverrides(overrides));
        }
        let mut seen = BTreeSet::new();
        let mut files = Vec::new();
        for file in raw.files {
            let reference = FileRef {
                project_id: file.project_id,
                file_id: file.file_id,
            };
            if !seen.insert(reference) {
                return Err(CurseForgeError::DuplicateFile {
                    project: file.project_id,
                    file: file.file_id,
                });
            }
            if file.required {
                files.push(reference);
            }
        }
        Ok(Self {
            name: raw.name,
            version: raw.version.filter(|version| !version.trim().is_empty()),
            game_version,
            loader,
            files,
            overrides,
        })
    }
}

/// The primary loader (or the only one); vanilla when the pack declares none.
fn loader(loaders: &[RawLoader]) -> Result<LoaderSpec, CurseForgeError> {
    let chosen = match loaders {
        [] => return Ok(LoaderSpec::vanilla()),
        [only] => only,
        _ => {
            let mut primaries = loaders.iter().filter(|loader| loader.primary);
            match (primaries.next(), primaries.next()) {
                (Some(primary), None) => primary,
                _ => return Err(CurseForgeError::SeveralLoaders),
            }
        }
    };
    let unsupported = || CurseForgeError::UnsupportedLoader(chosen.id.clone());
    let (name, version) = chosen.id.split_once('-').ok_or_else(unsupported)?;
    let kind = match name {
        "fabric" => LoaderKind::Fabric,
        "quilt" => LoaderKind::Quilt,
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::NeoForge,
        _ => return Err(unsupported()),
    };
    let version = LoaderVersion::parse(version).map_err(|_| unsupported())?;
    LoaderSpec::new(kind, Some(version)).map_err(|_| unsupported())
}

/// One path segment without separators or dot names.
fn is_safe_segment(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && RelativePath::parse(name).is_ok()
}

/// A file as the CurseForge API describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurseForgeFile {
    pub id: u64,
    pub project_id: u64,
    pub file_name: String,
    pub length: Option<u64>,
    /// Lowercase hex, when CurseForge declares one.
    pub sha1: Option<String>,
    /// `None` when the author forbids third-party distribution.
    pub download_url: Option<String>,
}

/// A project as the CurseForge API describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurseForgeProject {
    pub id: u64,
    pub name: String,
    pub class_id: Option<u64>,
}

/// What the launcher downloads, and what the player has to download.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedFiles {
    pub entries: Vec<PackEntry>,
    pub manual: Vec<ManualFile>,
}

/// Turns the pack's files into downloads (verified by sha1 and size, from
/// CurseForge's CDN only) or manual downloads (files without a download URL).
pub fn resolve_files(
    files: &[FileRef],
    api_files: &[CurseForgeFile],
    projects: &[CurseForgeProject],
) -> Result<ResolvedFiles, CurseForgeError> {
    let by_file: HashMap<u64, &CurseForgeFile> = api_files.iter().map(|f| (f.id, f)).collect();
    let by_project: HashMap<u64, &CurseForgeProject> = projects.iter().map(|p| (p.id, p)).collect();
    let mut targets = BTreeSet::new();
    let mut resolved = ResolvedFiles::default();
    for reference in files {
        let file = by_file
            .get(&reference.file_id)
            .ok_or(CurseForgeError::MissingFile(reference.file_id))?;
        if file.project_id != reference.project_id {
            return Err(CurseForgeError::WrongProject {
                project: reference.project_id,
                file: reference.file_id,
            });
        }
        let project = by_project
            .get(&reference.project_id)
            .ok_or(CurseForgeError::MissingProject(reference.project_id))?;
        let folder = CLASS_FOLDERS
            .iter()
            .find(|(class, _)| Some(*class) == project.class_id)
            .map(|(_, folder)| *folder)
            .ok_or(CurseForgeError::UnsupportedClass {
                project: project.id,
                class: project.class_id,
            })?;
        if !is_safe_segment(&file.file_name) {
            return Err(CurseForgeError::UnsafeFileName(file.file_name.clone()));
        }
        let path = RelativePath::parse(&format!("{folder}/{}", file.file_name))
            .map_err(|_| CurseForgeError::UnsafeFileName(file.file_name.clone()))?;
        let checksum = file
            .sha1
            .as_deref()
            .and_then(|sha1| Checksum::parse(HashAlgorithm::Sha1, sha1).ok())
            .ok_or_else(|| CurseForgeError::UnverifiedFile(file.file_name.clone()))?;
        if !targets.insert(path.as_str().to_lowercase()) {
            return Err(CurseForgeError::DuplicatePath(path.as_str().to_lowercase()));
        }
        match &file.download_url {
            Some(url) if is_allowed_download(url) => resolved.entries.push(PackEntry {
                path,
                source: EntrySource::Download {
                    urls: vec![url.clone()],
                    checksum,
                    size: file.length,
                },
                preserve: false,
            }),
            Some(url) => return Err(CurseForgeError::DisallowedDownload(url.clone())),
            None => resolved.manual.push(ManualFile {
                path,
                name: project.name.clone(),
                filename: file.file_name.clone(),
                checksum,
                project_id: project.id,
                file_id: file.id,
            }),
        }
    }
    Ok(resolved)
}

/// Whether `url` is an `https` URL on one of [`ALLOWED_DOWNLOAD_HOSTS`]
/// (no credentials, no explicit port).
pub fn is_allowed_download(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !authority.contains(['@', ':'])
        && ALLOWED_DOWNLOAD_HOSTS
            .iter()
            .any(|host| authority.eq_ignore_ascii_case(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";

    fn manifest() -> RawManifest {
        RawManifest {
            manifest_type: MANIFEST_TYPE.into(),
            manifest_version: 1,
            name: "Fabulously Optimized".into(),
            version: Some("14.1.1".into()),
            game_version: "26.2".into(),
            loaders: vec![RawLoader {
                id: "fabric-0.19.5".into(),
                primary: true,
            }],
            files: vec![
                RawFileRef {
                    project_id: 1,
                    file_id: 10,
                    required: true,
                },
                RawFileRef {
                    project_id: 2,
                    file_id: 20,
                    required: false,
                },
                RawFileRef {
                    project_id: 3,
                    file_id: 30,
                    required: true,
                },
            ],
            overrides: Some("overrides".into()),
        }
    }

    #[test]
    fn validates_a_manifest_and_keeps_only_required_files() {
        let pack = CurseForgePack::validate(manifest()).unwrap();
        assert_eq!(pack.name, "Fabulously Optimized");
        assert_eq!(pack.version.as_deref(), Some("14.1.1"));
        assert_eq!(pack.game_version.as_str(), "26.2");
        assert_eq!(pack.loader.kind(), LoaderKind::Fabric);
        assert_eq!(pack.loader.version().unwrap().as_str(), "0.19.5");
        assert_eq!(
            pack.files,
            [
                FileRef {
                    project_id: 1,
                    file_id: 10
                },
                FileRef {
                    project_id: 3,
                    file_id: 30
                }
            ]
        );
        assert_eq!(pack.overrides, "overrides");
    }

    #[test]
    fn reads_every_supported_loader_and_vanilla() {
        for (id, kind, version) in [
            ("forge-47.2.0", LoaderKind::Forge, "47.2.0"),
            ("neoforge-21.1.80", LoaderKind::NeoForge, "21.1.80"),
            ("quilt-0.26.0", LoaderKind::Quilt, "0.26.0"),
            ("fabric-0.19.5", LoaderKind::Fabric, "0.19.5"),
        ] {
            let mut raw = manifest();
            raw.loaders = vec![RawLoader {
                id: id.into(),
                primary: true,
            }];
            let pack = CurseForgePack::validate(raw).unwrap();
            assert_eq!(pack.loader.kind(), kind, "{id}");
            assert_eq!(pack.loader.version().unwrap().as_str(), version, "{id}");
        }
        let mut raw = manifest();
        raw.loaders.clear();
        assert_eq!(
            CurseForgePack::validate(raw).unwrap().loader,
            LoaderSpec::vanilla()
        );
        // A single loader counts even when not marked primary.
        let mut raw = manifest();
        raw.loaders[0].primary = false;
        assert_eq!(
            CurseForgePack::validate(raw).unwrap().loader.kind(),
            LoaderKind::Fabric
        );
        // Without overrides the folder is `overrides`.
        let mut raw = manifest();
        raw.overrides = None;
        assert_eq!(
            CurseForgePack::validate(raw).unwrap().overrides,
            "overrides"
        );
    }

    #[test]
    fn rejects_bad_manifests() {
        let check = |change: fn(&mut RawManifest), expected: CurseForgeError| {
            let mut raw = manifest();
            change(&mut raw);
            assert_eq!(CurseForgePack::validate(raw), Err(expected));
        };
        check(
            |m| m.manifest_type = "minecraftWorld".into(),
            CurseForgeError::UnsupportedManifest {
                kind: "minecraftWorld".into(),
                version: 1,
            },
        );
        check(
            |m| m.manifest_version = 2,
            CurseForgeError::UnsupportedManifest {
                kind: MANIFEST_TYPE.into(),
                version: 2,
            },
        );
        check(
            |m| m.game_version = "../x".into(),
            CurseForgeError::InvalidGameVersion("../x".into()),
        );
        check(
            |m| m.loaders[0].id = "liteloader-1.0".into(),
            CurseForgeError::UnsupportedLoader("liteloader-1.0".into()),
        );
        check(
            |m| m.loaders[0].id = "fabric".into(),
            CurseForgeError::UnsupportedLoader("fabric".into()),
        );
        check(
            |m| {
                m.loaders.push(RawLoader {
                    id: "forge-47.2.0".into(),
                    primary: true,
                })
            },
            CurseForgeError::SeveralLoaders,
        );
        check(
            |m| m.overrides = Some("../up".into()),
            CurseForgeError::UnsafeOverrides("../up".into()),
        );
        check(
            |m| m.overrides = Some("a/b".into()),
            CurseForgeError::UnsafeOverrides("a/b".into()),
        );
        check(
            |m| {
                m.files.push(RawFileRef {
                    project_id: 1,
                    file_id: 10,
                    required: true,
                })
            },
            CurseForgeError::DuplicateFile {
                project: 1,
                file: 10,
            },
        );
    }

    fn file(id: u64, project: u64, name: &str, url: Option<&str>) -> CurseForgeFile {
        CurseForgeFile {
            id,
            project_id: project,
            file_name: name.into(),
            length: Some(1234),
            sha1: Some(SHA1.into()),
            download_url: url.map(str::to_owned),
        }
    }

    fn project(id: u64, name: &str, class: u64) -> CurseForgeProject {
        CurseForgeProject {
            id,
            name: name.into(),
            class_id: Some(class),
        }
    }

    fn refs(ids: &[(u64, u64)]) -> Vec<FileRef> {
        ids.iter()
            .map(|&(project_id, file_id)| FileRef {
                project_id,
                file_id,
            })
            .collect()
    }

    const CDN: &str = "https://edge.forgecdn.net/files/1/2/";

    #[test]
    fn downloads_distributable_files_into_their_class_folder() {
        let resolved = resolve_files(
            &refs(&[(1, 10), (2, 20), (3, 30)]),
            &[
                file(10, 1, "sodium.jar", Some(&format!("{CDN}sodium.jar"))),
                file(
                    20,
                    2,
                    "Translations.zip",
                    Some(&format!("{CDN}Translations.zip")),
                ),
                file(30, 3, "shader.zip", Some(&format!("{CDN}shader.zip"))),
            ],
            &[
                project(1, "Sodium", 6),
                project(2, "Translations", 12),
                project(3, "Shader", 6552),
            ],
        )
        .unwrap();
        assert!(resolved.manual.is_empty());
        let paths: Vec<&str> = resolved.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "mods/sodium.jar",
                "resourcepacks/Translations.zip",
                "shaderpacks/shader.zip"
            ]
        );
        assert_eq!(
            resolved.entries[0].source,
            EntrySource::Download {
                urls: vec![format!("{CDN}sodium.jar")],
                checksum: Checksum::parse(HashAlgorithm::Sha1, SHA1).unwrap(),
                size: Some(1234),
            }
        );
        assert!(!resolved.entries[0].preserve);
    }

    #[test]
    fn files_without_a_download_url_become_manual_downloads() {
        let resolved = resolve_files(
            &refs(&[(448233, 9100313)]),
            &[file(
                9100313,
                448233,
                "entityculling-fabric-1.11.3-mc26.2.jar",
                None,
            )],
            &[project(448233, "Entity Culling", 6)],
        )
        .unwrap();
        assert!(resolved.entries.is_empty());
        assert_eq!(
            resolved.manual,
            [ManualFile {
                path: RelativePath::parse("mods/entityculling-fabric-1.11.3-mc26.2.jar").unwrap(),
                name: "Entity Culling".into(),
                filename: "entityculling-fabric-1.11.3-mc26.2.jar".into(),
                checksum: Checksum::parse(HashAlgorithm::Sha1, SHA1).unwrap(),
                project_id: 448233,
                file_id: 9100313,
            }]
        );
    }

    #[test]
    fn refuses_files_it_cannot_verify_or_place() {
        let one = refs(&[(1, 10)]);
        let sodium = project(1, "Sodium", 6);
        let ok_url = Some(format!("{CDN}a.jar"));
        let resolve = |f: CurseForgeFile, p: CurseForgeProject| resolve_files(&one, &[f], &[p]);

        assert_eq!(
            resolve_files(&one, &[], std::slice::from_ref(&sodium)),
            Err(CurseForgeError::MissingFile(10))
        );
        assert_eq!(
            resolve_files(&one, &[file(10, 1, "a.jar", ok_url.as_deref())], &[]),
            Err(CurseForgeError::MissingProject(1))
        );
        assert_eq!(
            resolve(file(10, 9, "a.jar", ok_url.as_deref()), sodium.clone()),
            Err(CurseForgeError::WrongProject {
                project: 1,
                file: 10
            })
        );
        assert_eq!(
            resolve(
                file(10, 1, "a.jar", ok_url.as_deref()),
                project(1, "World", 17)
            ),
            Err(CurseForgeError::UnsupportedClass {
                project: 1,
                class: Some(17)
            })
        );
        for bad in ["../a.jar", "a/b.jar", "a\\b.jar", "", ".."] {
            assert_eq!(
                resolve(file(10, 1, bad, ok_url.as_deref()), sodium.clone()),
                Err(CurseForgeError::UnsafeFileName(bad.into())),
                "{bad:?}"
            );
        }
        let mut no_hash = file(10, 1, "a.jar", ok_url.as_deref());
        no_hash.sha1 = None;
        assert_eq!(
            resolve(no_hash, sodium.clone()),
            Err(CurseForgeError::UnverifiedFile("a.jar".into()))
        );
        let mut bad_hash = file(10, 1, "a.jar", ok_url.as_deref());
        bad_hash.sha1 = Some("xyz".into());
        assert_eq!(
            resolve(bad_hash, sodium.clone()),
            Err(CurseForgeError::UnverifiedFile("a.jar".into()))
        );
        for url in [
            "http://edge.forgecdn.net/a.jar",
            "https://evil.example/a.jar",
            "https://edge.forgecdn.net:8443/a.jar",
        ] {
            assert_eq!(
                resolve(file(10, 1, "a.jar", Some(url)), sodium.clone()),
                Err(CurseForgeError::DisallowedDownload(url.into())),
                "{url}"
            );
        }
        // Two files landing on the same path (folders on macOS and Windows ignore case).
        assert_eq!(
            resolve_files(
                &refs(&[(1, 10), (1, 11)]),
                &[
                    file(10, 1, "A.jar", ok_url.as_deref()),
                    file(11, 1, "a.jar", ok_url.as_deref())
                ],
                &[sodium],
            ),
            Err(CurseForgeError::DuplicatePath("mods/a.jar".into()))
        );
    }

    #[test]
    fn allows_only_curseforge_cdn_hosts_over_https() {
        assert!(is_allowed_download(
            "https://edge.forgecdn.net/files/1/2/a.jar"
        ));
        assert!(is_allowed_download(
            "https://mediafilez.forgecdn.net/files/1/2/a.jar"
        ));
        assert!(is_allowed_download("https://EDGE.forgecdn.net/x"));
        for bad in [
            "http://edge.forgecdn.net/a",
            "https://edge.forgecdn.net.evil/a",
            "https://user@edge.forgecdn.net/a",
            "https://edge.forgecdn.net:443/a",
            "https://forgecdn.net/a",
            "edge.forgecdn.net/a",
        ] {
            assert!(!is_allowed_download(bad), "{bad}");
        }
    }
}
