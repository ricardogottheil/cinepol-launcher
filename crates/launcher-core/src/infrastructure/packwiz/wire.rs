//! The packwiz TOML formats (`pack.toml`, `index.toml`, `*.pw.toml`) as
//! strict serde types that ignore unknown keys, converted into the raw domain
//! types. Checking (safe paths, hashes, sides) happens in the domain.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::domain::modpack::PackFileError;
use crate::domain::modpack::packwiz::{RawIndexEntry, RawMetafile, RawPackIndex, RawPackToml};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct PackFile {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    pack_format: Option<String>,
    index: IndexRef,
    #[serde(default)]
    versions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct IndexRef {
    file: String,
    hash_format: String,
    hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct IndexFile {
    hash_format: String,
    #[serde(default)]
    files: Vec<IndexFileEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct IndexFileEntry {
    file: String,
    hash: String,
    #[serde(default)]
    hash_format: Option<String>,
    #[serde(default)]
    metafile: bool,
    #[serde(default)]
    preserve: bool,
    /// An alternative install path. The domain has no place for it yet, so
    /// the file installs at `file` and the alias is only logged.
    #[serde(default)]
    alias: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct MetaFile {
    name: String,
    filename: String,
    #[serde(default)]
    side: Option<String>,
    download: MetaDownload,
    #[serde(default)]
    update: Option<MetaUpdate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct MetaDownload {
    #[serde(default)]
    url: Option<String>,
    hash_format: String,
    hash: String,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MetaUpdate {
    #[serde(default)]
    curseforge: Option<CurseForgeUpdate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CurseForgeUpdate {
    file_id: u64,
    project_id: u64,
}

fn parse<T: serde::de::DeserializeOwned>(name: &str, bytes: &[u8]) -> Result<T, PackFileError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| PackFileError::Source(format!("{name} is not valid UTF-8")))?;
    toml::from_str(text).map_err(|error| {
        PackFileError::Source(format!("{name} is not a valid packwiz file: {error}"))
    })
}

/// Parses a `pack.toml`. `name` labels errors.
pub fn parse_pack(name: &str, bytes: &[u8]) -> Result<RawPackToml, PackFileError> {
    let file: PackFile = parse(name, bytes)?;
    Ok(RawPackToml {
        name: file.name,
        version: file.version,
        pack_format: file.pack_format,
        index_file: file.index.file,
        index_hash_format: file.index.hash_format,
        index_hash: file.index.hash,
        versions: file.versions,
    })
}

/// Parses an `index.toml`. `name` labels errors.
pub fn parse_index(name: &str, bytes: &[u8]) -> Result<RawPackIndex, PackFileError> {
    let file: IndexFile = parse(name, bytes)?;
    Ok(RawPackIndex {
        hash_format: file.hash_format,
        files: file
            .files
            .into_iter()
            .map(|entry| {
                if let Some(alias) = &entry.alias {
                    tracing::warn!(file = %entry.file, %alias, "packwiz aliases are not supported; installing at the listed path");
                }
                RawIndexEntry {
                    file: entry.file,
                    hash: entry.hash,
                    hash_format: entry.hash_format,
                    metafile: entry.metafile,
                    preserve: entry.preserve,
                }
            })
            .collect(),
    })
}

/// Parses a `*.pw.toml` metafile. `name` labels errors.
pub fn parse_metafile(name: &str, bytes: &[u8]) -> Result<RawMetafile, PackFileError> {
    let file: MetaFile = parse(name, bytes)?;
    Ok(RawMetafile {
        name: file.name,
        filename: file.filename,
        side: file.side,
        url: file.download.url,
        hash_format: file.download.hash_format,
        hash: file.download.hash,
        mode: file.download.mode,
        curseforge: file
            .update
            .and_then(|update| update.curseforge)
            .map(|cf| (cf.project_id, cf.file_id)),
    })
}
