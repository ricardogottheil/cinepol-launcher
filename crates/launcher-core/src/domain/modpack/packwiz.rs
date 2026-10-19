//! packwiz packs: `pack.toml` → `index.toml` → `*.pw.toml` metafiles.
//!
//! Adapters parse the TOML into the raw types below; the `validate`
//! functions turn them into checked values: safe paths, declared hashes,
//! client-side files only, and the game version and loader the pack needs.
//! Files in CurseForge mode carry no URL: they become [`ManualFile`]s the
//! player downloads by hand (the launcher verifies them with their hash).

use std::collections::BTreeMap;

use thiserror::Error;

use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::RelativePath;
use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};

use super::sync::ManualFile;

const MINECRAFT: &str = "minecraft";
/// The only packwiz format major version this launcher reads.
const FORMAT_MAJOR: &str = "1";
const METAFILE_SUFFIX: &str = ".pw.toml";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PackwizError {
    #[error("unsupported packwiz format {0:?}")]
    UnsupportedFormat(String),
    #[error("the pack does not declare a Minecraft version")]
    MissingGameVersion,
    #[error("invalid Minecraft version {0:?}")]
    InvalidGameVersion(String),
    #[error("unsupported pack loader {0:?}")]
    UnsupportedLoader(String),
    #[error("the pack declares more than one mod loader")]
    SeveralLoaders,
    #[error("invalid {loader} version {version:?}")]
    InvalidLoaderVersion { loader: String, version: String },
    #[error("unsafe path {0:?}")]
    UnsafePath(String),
    #[error("unsafe file name {0:?}")]
    UnsafeFileName(String),
    #[error("{0} is listed twice")]
    DuplicatePath(String),
    #[error("unsupported hash format {0:?}")]
    UnsupportedHash(String),
    #[error("{0} does not declare a valid hash")]
    InvalidHash(String),
    #[error("{0} is only protected by murmur2, which is trusted for CurseForge files only")]
    WeakHash(String),
    #[error("unsupported download mode {0:?}")]
    UnsupportedMode(String),
    #[error("{0} has no https download URL")]
    InvalidDownload(String),
    #[error("{0} is in CurseForge mode but has no project and file ids")]
    MissingCurseForgeIds(String),
    #[error("unknown side {0:?}")]
    InvalidSide(String),
}

/// `pack.toml` as written in the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawPackToml {
    pub name: String,
    pub version: Option<String>,
    /// `packwiz:1.1.0`; packs without it are format 1.0.
    pub pack_format: Option<String>,
    pub index_file: String,
    pub index_hash_format: String,
    pub index_hash: String,
    /// `minecraft`, `fabric`, `quilt`, `forge` or `neoforge` to a version.
    pub versions: BTreeMap<String, String>,
}

/// One `[[files]]` entry of `index.toml` as written in the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawIndexEntry {
    pub file: String,
    pub hash: String,
    /// Falls back to the index's `hash-format`.
    pub hash_format: Option<String>,
    pub metafile: bool,
    pub preserve: bool,
    /// Where the file is installed instead of `file` (packwiz `alias`).
    pub alias: Option<String>,
}

/// `index.toml` as written in the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawPackIndex {
    pub hash_format: String,
    pub files: Vec<RawIndexEntry>,
}

/// A `*.pw.toml` metafile as written in the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawMetafile {
    pub name: String,
    pub filename: String,
    pub side: Option<String>,
    pub url: Option<String>,
    pub hash_format: String,
    pub hash: String,
    /// Empty or `url` downloads `url`; `metadata:curseforge` resolves through CurseForge.
    pub mode: Option<String>,
    /// `[update.curseforge]` project-id and file-id.
    pub curseforge: Option<(u64, u64)>,
}

/// A checked `pack.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackwizPack {
    pub name: String,
    pub version: Option<String>,
    pub game_version: GameVersion,
    pub loader: LoaderSpec,
    /// The index location, relative to the folder holding `pack.toml`.
    pub index: RelativePath,
    pub index_checksum: Checksum,
}

impl PackwizPack {
    pub fn validate(raw: RawPackToml) -> Result<Self, PackwizError> {
        if let Some(format) = &raw.pack_format {
            let major = format
                .strip_prefix("packwiz:")
                .and_then(|version| version.split('.').next());
            if major != Some(FORMAT_MAJOR) {
                return Err(PackwizError::UnsupportedFormat(format.clone()));
            }
        }
        let (game_version, loader) = target(&raw.versions)?;
        let index = safe_path(&raw.index_file)?;
        let index_checksum = checksum(&raw.index_hash_format, &raw.index_hash, &raw.index_file)?;
        Ok(Self {
            name: raw.name,
            version: raw.version,
            game_version,
            loader,
            index,
            index_checksum,
        })
    }
}

/// One checked index entry. Paths are relative to the index's folder, which
/// is also where the file goes in the game folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    /// Where the entry is read from in the pack.
    pub path: RelativePath,
    /// Where it goes in the game folder: `path`, or the entry's `alias`.
    /// A metafile's file lands in this path's folder.
    pub target: RelativePath,
    pub checksum: Checksum,
    /// The entry is a `*.pw.toml` describing a file to download.
    pub metafile: bool,
    /// Written once; never replaced by a later update.
    pub preserve: bool,
}

/// A checked `index.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackwizIndex {
    pub entries: Vec<IndexEntry>,
}

impl PackwizIndex {
    pub fn validate(raw: RawPackIndex) -> Result<Self, PackwizError> {
        let mut seen = std::collections::BTreeSet::new();
        let mut entries = Vec::with_capacity(raw.files.len());
        for file in raw.files {
            let path = safe_path(&file.file)?;
            let target = match &file.alias {
                Some(alias) => safe_path(alias)?,
                None => path.clone(),
            };
            if !seen.insert(target.as_str().to_lowercase()) {
                return Err(PackwizError::DuplicatePath(file.alias.unwrap_or(file.file)));
            }
            let format = file.hash_format.as_deref().unwrap_or(&raw.hash_format);
            let checksum = checksum(format, &file.hash, &file.file)?;
            // Plain files and metafiles are fetched from the pack's own host.
            if checksum.algorithm() == HashAlgorithm::Murmur2 {
                return Err(PackwizError::WeakHash(file.file));
            }
            // packwiz marks metafiles explicitly; the suffix alone does not count.
            entries.push(IndexEntry {
                path,
                target,
                checksum,
                metafile: file.metafile,
                preserve: file.preserve,
            });
        }
        Ok(Self { entries })
    }
}

/// Where a metafile's file comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetafileSource {
    Download {
        url: String,
    },
    /// Needs the CurseForge API; until it can be used, the player downloads it.
    CurseForge(ManualFile),
}

/// A checked metafile that the client installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metafile {
    /// Where the file goes: the metafile's folder plus its `filename`.
    pub path: RelativePath,
    pub checksum: Checksum,
    pub source: MetafileSource,
}

impl Metafile {
    /// `None` for server-only files. `metafile_path` is the `*.pw.toml` path
    /// from the index; the file lands next to it.
    pub fn validate(
        raw: RawMetafile,
        metafile_path: &RelativePath,
    ) -> Result<Option<Self>, PackwizError> {
        match raw.side.as_deref() {
            None | Some("both") | Some("client") => {}
            Some("server") => return Ok(None),
            Some(other) => return Err(PackwizError::InvalidSide(other.to_owned())),
        }
        let filename = safe_file_name(&raw.filename)?;
        let folder = metafile_path
            .as_str()
            .rsplit_once('/')
            .map(|(folder, _)| folder);
        let path = safe_path(&match folder {
            Some(folder) => format!("{folder}/{filename}"),
            None => filename.to_owned(),
        })?;
        let checksum = checksum(&raw.hash_format, &raw.hash, &raw.filename)?;
        let source = match raw.mode.as_deref() {
            None | Some("") | Some("url") => {
                if checksum.algorithm() == HashAlgorithm::Murmur2 {
                    return Err(PackwizError::WeakHash(raw.filename));
                }
                let url = raw
                    .url
                    .filter(|url| url.starts_with("https://"))
                    .ok_or_else(|| PackwizError::InvalidDownload(raw.filename.clone()))?;
                MetafileSource::Download { url }
            }
            Some("metadata:curseforge") => {
                let (project_id, file_id) = raw
                    .curseforge
                    .ok_or_else(|| PackwizError::MissingCurseForgeIds(raw.filename.clone()))?;
                MetafileSource::CurseForge(ManualFile {
                    path: path.clone(),
                    name: raw.name,
                    filename: filename.to_owned(),
                    checksum: checksum.clone(),
                    project_id,
                    file_id,
                })
            }
            Some(other) => return Err(PackwizError::UnsupportedMode(other.to_owned())),
        };
        Ok(Some(Self {
            path,
            checksum,
            source,
        }))
    }
}

/// Whether an index path names a metafile by its suffix (for adapters that
/// need to tell them apart before parsing).
pub fn is_metafile_path(path: &RelativePath) -> bool {
    path.as_str().ends_with(METAFILE_SUFFIX)
}

fn safe_path(raw: &str) -> Result<RelativePath, PackwizError> {
    RelativePath::parse(raw).map_err(|_| PackwizError::UnsafePath(raw.to_owned()))
}

fn safe_file_name(raw: &str) -> Result<&str, PackwizError> {
    let single = RelativePath::parse(raw).is_ok() && !raw.contains('/');
    if single {
        Ok(raw)
    } else {
        Err(PackwizError::UnsafeFileName(raw.to_owned()))
    }
}

/// The algorithms packwiz declares that this launcher verifies.
fn algorithm(format: &str) -> Result<HashAlgorithm, PackwizError> {
    match format.to_ascii_lowercase().as_str() {
        "sha1" => Ok(HashAlgorithm::Sha1),
        "sha256" => Ok(HashAlgorithm::Sha256),
        "sha512" => Ok(HashAlgorithm::Sha512),
        "murmur2" => Ok(HashAlgorithm::Murmur2),
        _ => Err(PackwizError::UnsupportedHash(format.to_owned())),
    }
}

fn checksum(format: &str, value: &str, what: &str) -> Result<Checksum, PackwizError> {
    Checksum::parse(algorithm(format)?, value)
        .map_err(|_| PackwizError::InvalidHash(what.to_owned()))
}

fn loader_for_key(key: &str) -> Option<LoaderKind> {
    match key {
        "fabric" => Some(LoaderKind::Fabric),
        "quilt" => Some(LoaderKind::Quilt),
        "forge" => Some(LoaderKind::Forge),
        "neoforge" => Some(LoaderKind::NeoForge),
        _ => None,
    }
}

fn target(versions: &BTreeMap<String, String>) -> Result<(GameVersion, LoaderSpec), PackwizError> {
    let raw_game = versions
        .get(MINECRAFT)
        .ok_or(PackwizError::MissingGameVersion)?;
    let game_version = GameVersion::parse(raw_game)
        .map_err(|_| PackwizError::InvalidGameVersion(raw_game.clone()))?;
    let mut loader = LoaderSpec::vanilla();
    for (key, version) in versions {
        if key == MINECRAFT {
            continue;
        }
        let kind =
            loader_for_key(key).ok_or_else(|| PackwizError::UnsupportedLoader(key.clone()))?;
        if loader.kind() != LoaderKind::Vanilla {
            return Err(PackwizError::SeveralLoaders);
        }
        let invalid = || PackwizError::InvalidLoaderVersion {
            loader: key.clone(),
            version: version.clone(),
        };
        let parsed = LoaderVersion::parse(version).map_err(|_| invalid())?;
        loader = LoaderSpec::new(kind, Some(parsed)).map_err(|_| invalid())?;
    }
    Ok((game_version, loader))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA256: &str = "e08daa5b3e91b3fe3c98b3f9c279602f87e81c619c2741384939ac578671b025";
    const SHA1: &str = "c23406dbad87f9a7563c97ff3d5195311ab0fed6";

    fn pack(versions: &[(&str, &str)]) -> RawPackToml {
        RawPackToml {
            name: "Fabulously Optimized".into(),
            version: Some("14.1.1".into()),
            pack_format: Some("packwiz:1.1.0".into()),
            index_file: "index.toml".into(),
            index_hash_format: "sha256".into(),
            index_hash: SHA256.into(),
            versions: versions
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    fn path(raw: &str) -> RelativePath {
        RelativePath::parse(raw).unwrap()
    }

    #[test]
    fn validates_a_fabric_pack() {
        let pack =
            PackwizPack::validate(pack(&[("fabric", "0.19.5"), ("minecraft", "26.2")])).unwrap();
        assert_eq!(pack.game_version.as_str(), "26.2");
        assert_eq!(pack.loader.kind(), LoaderKind::Fabric);
        assert_eq!(pack.loader.version().unwrap().as_str(), "0.19.5");
        assert_eq!(pack.index.as_str(), "index.toml");
        assert_eq!(pack.index_checksum.algorithm(), HashAlgorithm::Sha256);
    }

    #[test]
    fn rejects_bad_targets_and_formats() {
        assert_eq!(
            PackwizPack::validate(pack(&[("fabric", "0.19.5")])),
            Err(PackwizError::MissingGameVersion)
        );
        assert_eq!(
            PackwizPack::validate(pack(&[("minecraft", "26.2"), ("liteloader", "1")])),
            Err(PackwizError::UnsupportedLoader("liteloader".into()))
        );
        assert_eq!(
            PackwizPack::validate(pack(&[
                ("minecraft", "26.2"),
                ("fabric", "1"),
                ("forge", "2")
            ])),
            Err(PackwizError::SeveralLoaders)
        );
        let mut future = pack(&[("minecraft", "26.2")]);
        future.pack_format = Some("packwiz:2.0.0".into());
        assert_eq!(
            PackwizPack::validate(future),
            Err(PackwizError::UnsupportedFormat("packwiz:2.0.0".into()))
        );
        let mut old = pack(&[("minecraft", "26.2")]);
        old.pack_format = None;
        assert!(PackwizPack::validate(old).is_ok());
        let mut escape = pack(&[("minecraft", "26.2")]);
        escape.index_file = "../index.toml".into();
        assert_eq!(
            PackwizPack::validate(escape),
            Err(PackwizError::UnsafePath("../index.toml".into()))
        );
    }

    fn entry(file: &str, hash: &str, format: Option<&str>) -> RawIndexEntry {
        RawIndexEntry {
            file: file.into(),
            hash: hash.into(),
            hash_format: format.map(str::to_owned),
            metafile: file.ends_with(".pw.toml"),
            preserve: false,
            alias: None,
        }
    }

    #[test]
    fn index_entries_use_their_own_or_the_index_hash_format() {
        let index = PackwizIndex::validate(RawPackIndex {
            hash_format: "sha256".into(),
            files: vec![
                entry("config/a.toml", SHA256, None),
                entry("mods/jei.pw.toml", SHA1, Some("sha1")),
                entry("config/b.json", &"a".repeat(128), Some("sha512")),
            ],
        })
        .unwrap();
        let algorithms: Vec<_> = index
            .entries
            .iter()
            .map(|e| e.checksum.algorithm())
            .collect();
        assert_eq!(
            algorithms,
            [
                HashAlgorithm::Sha256,
                HashAlgorithm::Sha1,
                HashAlgorithm::Sha512
            ]
        );
        assert!(index.entries[1].metafile && !index.entries[0].metafile);
    }

    #[test]
    fn an_alias_is_where_the_entry_is_installed() {
        let validate = |files| {
            PackwizIndex::validate(RawPackIndex {
                hash_format: "sha256".into(),
                files,
            })
        };
        let aliased = |file: &str, alias: &str| RawIndexEntry {
            alias: Some(alias.into()),
            ..entry(file, SHA256, None)
        };
        let index = validate(vec![
            aliased("pack/options.txt", "options.txt"),
            entry("config/a.toml", SHA256, None),
        ])
        .unwrap();
        assert_eq!(index.entries[0].path.as_str(), "pack/options.txt");
        assert_eq!(index.entries[0].target.as_str(), "options.txt");
        assert_eq!(index.entries[1].target.as_str(), "config/a.toml");
        assert_eq!(
            validate(vec![aliased("a.txt", "../a.txt")]),
            Err(PackwizError::UnsafePath("../a.txt".into()))
        );
        assert_eq!(
            validate(vec![
                entry("config/a.toml", SHA256, None),
                aliased("other/a.toml", "Config/A.toml")
            ]),
            Err(PackwizError::DuplicatePath("Config/A.toml".into()))
        );
    }

    #[test]
    fn index_rejects_unsafe_duplicate_and_unverifiable_entries() {
        let validate = |files| {
            PackwizIndex::validate(RawPackIndex {
                hash_format: "sha256".into(),
                files,
            })
        };
        for bad in ["../x", "/etc/passwd", "a\\b", "C:/x"] {
            assert_eq!(
                validate(vec![entry(bad, SHA256, None)]),
                Err(PackwizError::UnsafePath(bad.into())),
                "{bad}"
            );
        }
        assert_eq!(
            validate(vec![
                entry("Config/A.toml", SHA256, None),
                entry("config/a.toml", SHA256, None)
            ]),
            Err(PackwizError::DuplicatePath("config/a.toml".into()))
        );
        assert_eq!(
            validate(vec![entry("config/a.toml", "abc", None)]),
            Err(PackwizError::InvalidHash("config/a.toml".into()))
        );
        assert_eq!(
            validate(vec![entry("config/a.toml", "abc", Some("md5"))]),
            Err(PackwizError::UnsupportedHash("md5".into()))
        );
    }

    fn metafile(side: Option<&str>, mode: Option<&str>) -> RawMetafile {
        RawMetafile {
            name: "Just Enough Items (JEI)".into(),
            filename: "jei-1.20.1-forge-15.20.0.133.jar".into(),
            side: side.map(str::to_owned),
            url: Some("https://cdn.modrinth.com/data/u6dRKJwZ/versions/x/jei.jar".into()),
            hash_format: "sha1".into(),
            hash: SHA1.into(),
            mode: mode.map(str::to_owned),
            curseforge: Some((238_222, 8_367_520)),
        }
    }

    #[test]
    fn url_metafiles_download_next_to_the_metafile() {
        let file = Metafile::validate(metafile(Some("both"), None), &path("mods/jei.pw.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(file.path.as_str(), "mods/jei-1.20.1-forge-15.20.0.133.jar");
        assert_eq!(file.checksum.value(), SHA1);
        assert_eq!(
            file.source,
            MetafileSource::Download {
                url: "https://cdn.modrinth.com/data/u6dRKJwZ/versions/x/jei.jar".into()
            }
        );
        let root = Metafile::validate(metafile(None, Some("url")), &path("jei.pw.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(root.path.as_str(), "jei-1.20.1-forge-15.20.0.133.jar");
    }

    #[test]
    fn curseforge_metafiles_become_manual_downloads() {
        let file = Metafile::validate(
            metafile(Some("client"), Some("metadata:curseforge")),
            &path("mods/jei.pw.toml"),
        )
        .unwrap()
        .unwrap();
        let MetafileSource::CurseForge(manual) = file.source else {
            panic!("expected a manual file");
        };
        assert_eq!(manual.project_id, 238_222);
        assert_eq!(manual.file_id, 8_367_520);
        assert_eq!(manual.filename, "jei-1.20.1-forge-15.20.0.133.jar");
        assert_eq!(manual.path, file.path);
        assert_eq!(
            manual.page_url(),
            "https://www.curseforge.com/projects/238222"
        );
        let mut no_ids = metafile(None, Some("metadata:curseforge"));
        no_ids.curseforge = None;
        assert!(matches!(
            Metafile::validate(no_ids, &path("mods/jei.pw.toml")),
            Err(PackwizError::MissingCurseForgeIds(_))
        ));
    }

    #[test]
    fn metafiles_skip_server_files_and_refuse_unsafe_ones() {
        assert_eq!(
            Metafile::validate(metafile(Some("server"), None), &path("mods/jei.pw.toml")),
            Ok(None)
        );
        assert_eq!(
            Metafile::validate(
                metafile(Some("everywhere"), None),
                &path("mods/jei.pw.toml")
            ),
            Err(PackwizError::InvalidSide("everywhere".into()))
        );
        for name in ["../evil.jar", "sub/evil.jar", "", "C:evil.jar"] {
            let mut raw = metafile(None, None);
            raw.filename = name.into();
            assert_eq!(
                Metafile::validate(raw, &path("mods/jei.pw.toml")),
                Err(PackwizError::UnsafeFileName(name.into())),
                "{name}"
            );
        }
        let mut http = metafile(None, None);
        http.url = Some("http://example.com/jei.jar".into());
        assert!(matches!(
            Metafile::validate(http, &path("mods/jei.pw.toml")),
            Err(PackwizError::InvalidDownload(_))
        ));
        assert_eq!(
            Metafile::validate(metafile(None, Some("torrent")), &path("mods/jei.pw.toml")),
            Err(PackwizError::UnsupportedMode("torrent".into()))
        );
    }

    #[test]
    fn murmur2_is_only_trusted_for_curseforge_files() {
        // A 32-bit non-cryptographic hash cannot protect a file fetched from a URL.
        let mut url_mode = metafile(None, None);
        url_mode.hash_format = "murmur2".into();
        url_mode.hash = "3608199863".into();
        assert_eq!(
            Metafile::validate(url_mode, &path("mods/jei.pw.toml")),
            Err(PackwizError::WeakHash(
                "jei-1.20.1-forge-15.20.0.133.jar".into()
            ))
        );
        let mut curseforge = metafile(None, Some("metadata:curseforge"));
        curseforge.hash_format = "murmur2".into();
        curseforge.hash = "3608199863".into();
        assert!(Metafile::validate(curseforge, &path("mods/jei.pw.toml")).is_ok());
        assert_eq!(
            PackwizIndex::validate(RawPackIndex {
                hash_format: "sha256".into(),
                files: vec![entry("config/b.json", "3608199863", Some("murmur2"))],
            }),
            Err(PackwizError::WeakHash("config/b.json".into()))
        );
    }
}
