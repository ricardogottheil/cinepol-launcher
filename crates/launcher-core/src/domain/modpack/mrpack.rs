//! The Modrinth modpack format (`.mrpack`): a zip holding `modrinth.index.json`
//! plus `overrides/`, `client-overrides/` and `server-overrides/` folders.
//!
//! Adapters deserialize the index into the raw types below; [`MrpackIndex::validate`]
//! turns them into the checked model use cases work with.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::RelativePath;
use crate::domain::loader::{LoaderKind, LoaderSpec, LoaderVersion};

/// The only index format version this launcher reads and writes.
pub const MRPACK_FORMAT_VERSION: u32 = 1;

/// Name of the index file at the root of the archive.
pub const MRPACK_INDEX: &str = "modrinth.index.json";

/// Hosts the mrpack specification allows downloads from.
pub const ALLOWED_DOWNLOAD_HOSTS: [&str; 4] = [
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
];

/// Top-level entries of the game folder that an export packs.
pub const EXPORT_ROOTS: [&str; 5] = [
    "mods",
    "config",
    "resourcepacks",
    "shaderpacks",
    "options.txt",
];

const MINECRAFT: &str = "minecraft";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MrpackError {
    #[error("unsupported mrpack format version {0}")]
    UnsupportedFormat(u32),
    #[error("the pack is for {0:?}, not Minecraft")]
    NotMinecraft(String),
    #[error("the pack does not declare a Minecraft version")]
    MissingGameVersion,
    #[error("invalid Minecraft version {0:?}")]
    InvalidGameVersion(String),
    #[error("unsupported pack dependency {0:?}")]
    UnsupportedDependency(String),
    #[error("the pack declares more than one mod loader")]
    SeveralLoaders,
    #[error("invalid {loader} version {version:?}")]
    InvalidLoaderVersion { loader: String, version: String },
    #[error("unsafe file path {0:?}")]
    UnsafePath(String),
    #[error("{0} is listed twice")]
    DuplicatePath(String),
    #[error("{0} does not declare valid sha1 and sha512 hashes")]
    UnverifiedFile(String),
    #[error("{0} has no download URL")]
    NoDownload(String),
    #[error("download host not allowed: {0}")]
    DisallowedDownload(String),
    #[error("unknown env value {0:?}")]
    InvalidEnv(String),
}

/// Whether a side (client or server) needs a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvSupport {
    Required,
    Optional,
    Unsupported,
}

impl EnvSupport {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "required" => Some(Self::Required),
            "optional" => Some(Self::Optional),
            "unsupported" => Some(Self::Unsupported),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Unsupported => "unsupported",
        }
    }
}

/// `env` of an index entry as written in the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawMrpackEnv {
    pub client: Option<String>,
    pub server: Option<String>,
}

/// One `files[]` entry as written in the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawMrpackFile {
    pub path: String,
    /// Algorithm name (`sha1`, `sha512`, ...) to hex digest.
    pub hashes: BTreeMap<String, String>,
    pub env: Option<RawMrpackEnv>,
    pub downloads: Vec<String>,
    pub file_size: u64,
}

/// `modrinth.index.json` as written in the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawMrpackIndex {
    pub format_version: u32,
    pub game: String,
    pub version_id: String,
    pub name: String,
    pub summary: Option<String>,
    pub files: Vec<RawMrpackFile>,
    /// `minecraft`, `forge`, `neoforge`, `fabric-loader` or `quilt-loader` to a version.
    pub dependencies: BTreeMap<String, String>,
}

/// A checked index entry: a safe path, both hashes and allowed download URLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MrpackFile {
    pub path: RelativePath,
    pub sha1: Checksum,
    pub sha512: Checksum,
    pub size: u64,
    pub downloads: Vec<String>,
    pub client: EnvSupport,
    pub server: EnvSupport,
}

impl MrpackFile {
    /// Whether the client installs this file (required and optional ones are).
    pub fn for_client(&self) -> bool {
        self.client != EnvSupport::Unsupported
    }
}

/// A checked `modrinth.index.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MrpackIndex {
    pub name: String,
    pub version_id: String,
    pub summary: Option<String>,
    pub game_version: GameVersion,
    pub loader: LoaderSpec,
    pub files: Vec<MrpackFile>,
}

impl MrpackIndex {
    pub fn validate(raw: RawMrpackIndex) -> Result<Self, MrpackError> {
        if raw.format_version != MRPACK_FORMAT_VERSION {
            return Err(MrpackError::UnsupportedFormat(raw.format_version));
        }
        if raw.game != MINECRAFT {
            return Err(MrpackError::NotMinecraft(raw.game));
        }
        let (game_version, loader) = target(&raw.dependencies)?;
        let mut seen = BTreeSet::new();
        let mut files = Vec::with_capacity(raw.files.len());
        for file in raw.files {
            let file = validate_file(file)?;
            if !seen.insert(file.path.clone()) {
                return Err(MrpackError::DuplicatePath(file.path.to_string()));
            }
            files.push(file);
        }
        Ok(Self {
            name: raw.name,
            version_id: raw.version_id,
            summary: raw.summary,
            game_version,
            loader,
            files,
        })
    }

    /// The entries the client installs, skipping server-only ones.
    pub fn client_files(&self) -> impl Iterator<Item = &MrpackFile> {
        self.files.iter().filter(|file| file.for_client())
    }

    /// The raw form, for writing the index of an export.
    pub fn to_raw(&self) -> RawMrpackIndex {
        let mut dependencies = BTreeMap::new();
        dependencies.insert(MINECRAFT.to_owned(), self.game_version.as_str().to_owned());
        if let (Some(key), Some(version)) =
            (dependency_key(self.loader.kind()), self.loader.version())
        {
            dependencies.insert(key.to_owned(), version.to_string());
        }
        RawMrpackIndex {
            format_version: MRPACK_FORMAT_VERSION,
            game: MINECRAFT.to_owned(),
            version_id: self.version_id.clone(),
            name: self.name.clone(),
            summary: self.summary.clone(),
            files: self.files.iter().map(raw_file).collect(),
            dependencies,
        }
    }
}

/// The mrpack dependency key of a loader (`None` for vanilla).
pub fn dependency_key(kind: LoaderKind) -> Option<&'static str> {
    match kind {
        LoaderKind::Vanilla => None,
        LoaderKind::Fabric => Some("fabric-loader"),
        LoaderKind::Quilt => Some("quilt-loader"),
        LoaderKind::Forge => Some("forge"),
        LoaderKind::NeoForge => Some("neoforge"),
    }
}

fn loader_for_key(key: &str) -> Option<LoaderKind> {
    [
        LoaderKind::Fabric,
        LoaderKind::Quilt,
        LoaderKind::Forge,
        LoaderKind::NeoForge,
    ]
    .into_iter()
    .find(|kind| dependency_key(*kind) == Some(key))
}

fn target(
    dependencies: &BTreeMap<String, String>,
) -> Result<(GameVersion, LoaderSpec), MrpackError> {
    let raw_game = dependencies
        .get(MINECRAFT)
        .ok_or(MrpackError::MissingGameVersion)?;
    let game_version = GameVersion::parse(raw_game)
        .map_err(|_| MrpackError::InvalidGameVersion(raw_game.clone()))?;
    let mut loader = LoaderSpec::vanilla();
    for (key, version) in dependencies {
        if key == MINECRAFT {
            continue;
        }
        let kind =
            loader_for_key(key).ok_or_else(|| MrpackError::UnsupportedDependency(key.clone()))?;
        if loader.kind() != LoaderKind::Vanilla {
            return Err(MrpackError::SeveralLoaders);
        }
        let invalid = || MrpackError::InvalidLoaderVersion {
            loader: key.clone(),
            version: version.clone(),
        };
        let version = LoaderVersion::parse(version).map_err(|_| invalid())?;
        loader = LoaderSpec::new(kind, Some(version)).map_err(|_| invalid())?;
    }
    Ok((game_version, loader))
}

fn validate_file(raw: RawMrpackFile) -> Result<MrpackFile, MrpackError> {
    let path =
        RelativePath::parse(&raw.path).map_err(|_| MrpackError::UnsafePath(raw.path.clone()))?;
    let hash = |algorithm: HashAlgorithm, name: &str| {
        raw.hashes
            .get(name)
            .and_then(|value| Checksum::parse(algorithm, value).ok())
            .ok_or_else(|| MrpackError::UnverifiedFile(raw.path.clone()))
    };
    let sha1 = hash(HashAlgorithm::Sha1, "sha1")?;
    let sha512 = hash(HashAlgorithm::Sha512, "sha512")?;
    if raw.downloads.is_empty() {
        return Err(MrpackError::NoDownload(raw.path));
    }
    if let Some(url) = raw.downloads.iter().find(|url| !is_allowed_download(url)) {
        return Err(MrpackError::DisallowedDownload(url.clone()));
    }
    let env = raw.env.unwrap_or_default();
    let side = |value: Option<String>| match value {
        None => Ok(EnvSupport::Required),
        Some(value) => EnvSupport::parse(&value).ok_or(MrpackError::InvalidEnv(value)),
    };
    Ok(MrpackFile {
        path,
        sha1,
        sha512,
        size: raw.file_size,
        downloads: raw.downloads,
        client: side(env.client)?,
        server: side(env.server)?,
    })
}

fn raw_file(file: &MrpackFile) -> RawMrpackFile {
    let mut hashes = BTreeMap::new();
    hashes.insert("sha1".to_owned(), file.sha1.value().to_owned());
    hashes.insert("sha512".to_owned(), file.sha512.value().to_owned());
    RawMrpackFile {
        path: file.path.to_string(),
        hashes,
        env: Some(RawMrpackEnv {
            client: Some(file.client.id().to_owned()),
            server: Some(file.server.id().to_owned()),
        }),
        downloads: file.downloads.clone(),
        file_size: file.size,
    }
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

/// Whether an export includes `path` (see [`EXPORT_ROOTS`]).
pub fn is_exportable(path: &RelativePath) -> bool {
    let first = path.as_str().split('/').next().unwrap_or_default();
    let is_file = !path.as_str().contains('/');
    EXPORT_ROOTS
        .iter()
        .any(|root| *root == first && (*root != "options.txt" || is_file))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA1: &str = "0f4a890d07402280686a518579fa9fa02309e315";
    const SHA512: &str = "7b70e796cea2ee57a6022108092517b6aa39d5a19b8da1de26685ae53b8fbb4717cd91ee1d232c54efa8e210ebe9b760d3dab79bf9a5fdb5ae75d97b461a1fab";

    fn file(path: &str) -> RawMrpackFile {
        RawMrpackFile {
            path: path.into(),
            hashes: [
                ("sha1".into(), SHA1.into()),
                ("sha512".into(), SHA512.into()),
            ]
            .into(),
            env: None,
            downloads: vec![format!("https://cdn.modrinth.com/data/x/versions/y/{path}")],
            file_size: 10,
        }
    }

    fn index(files: Vec<RawMrpackFile>, dependencies: &[(&str, &str)]) -> RawMrpackIndex {
        RawMrpackIndex {
            format_version: 1,
            game: "minecraft".into(),
            version_id: "14.1.1".into(),
            name: "Fabulously Optimized".into(),
            summary: None,
            files,
            dependencies: dependencies
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    fn fabric_index(files: Vec<RawMrpackFile>) -> RawMrpackIndex {
        index(files, &[("minecraft", "26.2"), ("fabric-loader", "0.19.5")])
    }

    #[test]
    fn validates_a_fabric_pack_and_maps_its_dependencies() {
        let pack = MrpackIndex::validate(fabric_index(vec![file("mods/a.jar")])).unwrap();
        assert_eq!(pack.game_version.as_str(), "26.2");
        assert_eq!(pack.loader.kind(), LoaderKind::Fabric);
        assert_eq!(pack.loader.version().unwrap().as_str(), "0.19.5");
        assert_eq!(pack.files[0].path.as_str(), "mods/a.jar");
        assert_eq!(pack.files[0].sha512.value(), SHA512);
        assert_eq!(pack.files[0].client, EnvSupport::Required);
    }

    #[test]
    fn maps_every_loader_key_and_vanilla() {
        for (key, kind) in [
            ("quilt-loader", LoaderKind::Quilt),
            ("forge", LoaderKind::Forge),
            ("neoforge", LoaderKind::NeoForge),
        ] {
            let pack = MrpackIndex::validate(index(vec![], &[("minecraft", "26.2"), (key, "1.0")]))
                .unwrap();
            assert_eq!(pack.loader.kind(), kind);
        }
        let vanilla = MrpackIndex::validate(index(vec![], &[("minecraft", "26.2")])).unwrap();
        assert_eq!(vanilla.loader, LoaderSpec::vanilla());
    }

    #[test]
    fn rejects_bad_targets() {
        let cases = [
            (
                index(vec![], &[("fabric-loader", "0.19.5")]),
                MrpackError::MissingGameVersion,
            ),
            (
                index(vec![], &[("minecraft", "26.2"), ("liteloader", "1")]),
                MrpackError::UnsupportedDependency("liteloader".into()),
            ),
            (
                index(
                    vec![],
                    &[
                        ("minecraft", "26.2"),
                        ("fabric-loader", "1"),
                        ("forge", "2"),
                    ],
                ),
                MrpackError::SeveralLoaders,
            ),
        ];
        for (raw, expected) in cases {
            assert_eq!(MrpackIndex::validate(raw), Err(expected));
        }
        let mut other_game = fabric_index(vec![]);
        other_game.game = "terraria".into();
        assert_eq!(
            MrpackIndex::validate(other_game),
            Err(MrpackError::NotMinecraft("terraria".into()))
        );
        let mut future = fabric_index(vec![]);
        future.format_version = 2;
        assert_eq!(
            MrpackIndex::validate(future),
            Err(MrpackError::UnsupportedFormat(2))
        );
    }

    #[test]
    fn rejects_paths_that_escape_the_game_folder() {
        for path in [
            "../evil.jar",
            "/etc/passwd",
            "mods/../../x",
            "C:/x.jar",
            "mods\\x.jar",
            "",
        ] {
            assert_eq!(
                MrpackIndex::validate(fabric_index(vec![file(path)])),
                Err(MrpackError::UnsafePath(path.into())),
                "{path}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_paths() {
        assert_eq!(
            MrpackIndex::validate(fabric_index(vec![file("mods/a.jar"), file("mods/a.jar")])),
            Err(MrpackError::DuplicatePath("mods/a.jar".into()))
        );
    }

    #[test]
    fn refuses_files_without_both_valid_hashes() {
        for (name, value) in [("sha1", "not-hex"), ("sha512", "abc")] {
            let mut entry = file("mods/a.jar");
            entry.hashes.insert(name.into(), value.into());
            assert_eq!(
                MrpackIndex::validate(fabric_index(vec![entry])),
                Err(MrpackError::UnverifiedFile("mods/a.jar".into()))
            );
        }
        let mut entry = file("mods/a.jar");
        entry.hashes.remove("sha512");
        assert_eq!(
            MrpackIndex::validate(fabric_index(vec![entry])),
            Err(MrpackError::UnverifiedFile("mods/a.jar".into()))
        );
    }

    #[test]
    fn refuses_downloads_outside_the_allowed_hosts() {
        let mut entry = file("mods/a.jar");
        entry.downloads = vec!["https://evil.example/a.jar".into()];
        assert_eq!(
            MrpackIndex::validate(fabric_index(vec![entry])),
            Err(MrpackError::DisallowedDownload(
                "https://evil.example/a.jar".into()
            ))
        );
        let mut none = file("mods/b.jar");
        none.downloads.clear();
        assert_eq!(
            MrpackIndex::validate(fabric_index(vec![none])),
            Err(MrpackError::NoDownload("mods/b.jar".into()))
        );
    }

    #[test]
    fn allows_only_https_on_listed_hosts() {
        assert!(is_allowed_download("https://cdn.modrinth.com/data/a/b.jar"));
        assert!(is_allowed_download(
            "https://GitHub.com/o/r/releases/download/a.jar"
        ));
        for url in [
            "http://cdn.modrinth.com/a.jar",
            "https://cdn.modrinth.com.evil.example/a.jar",
            "https://user@cdn.modrinth.com/a.jar",
            "https://cdn.modrinth.com:8443/a.jar",
            "https://evil.example/?https://cdn.modrinth.com/",
        ] {
            assert!(!is_allowed_download(url), "{url}");
        }
    }

    #[test]
    fn client_files_skip_server_only_entries_and_keep_optional_ones() {
        let side = |path: &str, client: &str| {
            let mut entry = file(path);
            entry.env = Some(RawMrpackEnv {
                client: Some(client.into()),
                server: Some("required".into()),
            });
            entry
        };
        let pack = MrpackIndex::validate(fabric_index(vec![
            side("mods/both.jar", "required"),
            side("mods/optional.jar", "optional"),
            side("mods/server.jar", "unsupported"),
        ]))
        .unwrap();
        let paths: Vec<&str> = pack.client_files().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["mods/both.jar", "mods/optional.jar"]);
        let mut bad = file("mods/x.jar");
        bad.env = Some(RawMrpackEnv {
            client: Some("sometimes".into()),
            server: None,
        });
        assert_eq!(
            MrpackIndex::validate(fabric_index(vec![bad])),
            Err(MrpackError::InvalidEnv("sometimes".into()))
        );
    }

    #[test]
    fn round_trips_through_the_raw_form() {
        let pack = MrpackIndex::validate(fabric_index(vec![file("mods/a.jar")])).unwrap();
        let raw = pack.to_raw();
        assert_eq!(raw.dependencies["minecraft"], "26.2");
        assert_eq!(raw.dependencies["fabric-loader"], "0.19.5");
        assert_eq!(MrpackIndex::validate(raw).unwrap(), pack);
    }

    #[test]
    fn exports_only_the_listed_roots() {
        let path = |raw: &str| RelativePath::parse(raw).unwrap();
        for included in [
            "mods/a.jar",
            "config/x.toml",
            "resourcepacks/p.zip",
            "shaderpacks/s.zip",
            "options.txt",
        ] {
            assert!(is_exportable(&path(included)), "{included}");
        }
        for excluded in [
            "saves/world/level.dat",
            "logs/latest.log",
            "options.txt/x",
            "modsx/a.jar",
            "servers.dat",
        ] {
            assert!(!is_exportable(&path(excluded)), "{excluded}");
        }
    }
}
