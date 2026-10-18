//! `modrinth.index.json` as written in the file (camelCase), converted to and
//! from the domain's raw index. Unknown fields are ignored.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::modpack::mrpack::{RawMrpackEnv, RawMrpackFile, RawMrpackIndex};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireIndex {
    format_version: u32,
    game: String,
    version_id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(default)]
    files: Vec<WireFile>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireFile {
    path: String,
    #[serde(default)]
    hashes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    env: Option<WireEnv>,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    file_size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct WireEnv {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    server: Option<String>,
}

impl From<WireIndex> for RawMrpackIndex {
    fn from(wire: WireIndex) -> Self {
        Self {
            format_version: wire.format_version,
            game: wire.game,
            version_id: wire.version_id,
            name: wire.name,
            summary: wire.summary,
            files: wire
                .files
                .into_iter()
                .map(|file| RawMrpackFile {
                    path: file.path,
                    hashes: file.hashes,
                    env: file.env.map(|env| RawMrpackEnv {
                        client: env.client,
                        server: env.server,
                    }),
                    downloads: file.downloads,
                    file_size: file.file_size,
                })
                .collect(),
            dependencies: wire.dependencies,
        }
    }
}

impl From<RawMrpackIndex> for WireIndex {
    fn from(raw: RawMrpackIndex) -> Self {
        Self {
            format_version: raw.format_version,
            game: raw.game,
            version_id: raw.version_id,
            name: raw.name,
            summary: raw.summary,
            files: raw
                .files
                .into_iter()
                .map(|file| WireFile {
                    path: file.path,
                    hashes: file.hashes,
                    env: file.env.map(|env| WireEnv {
                        client: env.client,
                        server: env.server,
                    }),
                    downloads: file.downloads,
                    file_size: file.file_size,
                })
                .collect(),
            dependencies: raw.dependencies,
        }
    }
}
