//! CurseForge API v1 and `manifest.json` documents. Unknown fields are
//! ignored; optional data may be missing.

use serde::{Deserialize, Serialize};

/// `{ "data": ... }`, the envelope of every answer.
#[derive(Debug, Deserialize)]
pub struct Data<T> {
    pub data: T,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub data: Vec<Mod>,
    pub pagination: Option<Pagination>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    pub total_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub class_id: Option<u64>,
    #[serde(default)]
    pub download_count: Option<f64>,
    #[serde(default)]
    pub logo: Option<Logo>,
    /// The newest file per game version and loader.
    #[serde(default)]
    pub latest_files_indexes: Vec<FileIndex>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIndex {
    #[serde(default)]
    pub mod_loader: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logo {
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub id: u64,
    pub mod_id: u64,
    #[serde(default)]
    pub display_name: Option<String>,
    pub file_name: String,
    #[serde(default)]
    pub file_length: Option<u64>,
    #[serde(default)]
    pub hashes: Vec<Hash>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub file_date: Option<String>,
    #[serde(default)]
    pub is_server_pack: Option<bool>,
}

/// `algo` 1 is sha1, 2 is md5.
#[derive(Debug, Deserialize)]
pub struct Hash {
    pub value: String,
    pub algo: u32,
}

pub const SHA1_ALGO: u32 = 1;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesRequest<'a> {
    pub file_ids: &'a [u64],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsRequest<'a> {
    pub mod_ids: &'a [u64],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub minecraft: Minecraft,
    pub manifest_type: String,
    pub manifest_version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub files: Vec<ManifestFile>,
    #[serde(default)]
    pub overrides: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Minecraft {
    pub version: String,
    #[serde(default)]
    pub mod_loaders: Vec<ModLoader>,
}

#[derive(Debug, Deserialize)]
pub struct ModLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    /// Files are required unless they say otherwise.
    #[serde(default = "required")]
    pub required: bool,
}

fn required() -> bool {
    true
}
