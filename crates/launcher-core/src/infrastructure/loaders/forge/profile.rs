//! `install_profile.json` (spec 1, Forge 1.13+ and NeoForge) and the
//! libraries it and `version.json` list.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::launch::{Artifact, MavenCoordinate, RelativePath};
use crate::domain::loader::{LoaderInstallError, LoaderKind, ProcessorSpec};

/// The only install profile format this launcher runs.
const SUPPORTED_SPEC: u64 = 1;

/// What the launcher needs from an install profile.
#[derive(Debug)]
pub(super) struct InstallProfile {
    /// The version id the profile installs.
    pub version: String,
    pub minecraft: String,
    /// Where `version.json` is inside the installer.
    pub json: RelativePath,
    /// The client value of every `data` entry.
    pub data: BTreeMap<String, String>,
    pub processors: Vec<ProcessorSpec>,
    /// The processor tools, each with a sha1.
    pub libraries: Vec<Artifact>,
}

#[derive(Debug, Deserialize)]
struct RawProfile {
    version: String,
    minecraft: String,
    json: String,
    #[serde(default)]
    data: BTreeMap<String, RawSided>,
    #[serde(default)]
    processors: Vec<RawProcessor>,
    #[serde(default)]
    libraries: Vec<RawLibrary>,
}

#[derive(Debug, Deserialize)]
struct RawSided {
    client: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawProcessor {
    jar: String,
    #[serde(default)]
    classpath: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    outputs: BTreeMap<String, String>,
    sides: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RawLibrary {
    name: String,
    downloads: Option<RawDownloads>,
}

#[derive(Debug, Deserialize)]
struct RawDownloads {
    artifact: Option<RawArtifact>,
}

#[derive(Debug, Deserialize)]
struct RawArtifact {
    path: Option<String>,
    #[serde(default)]
    url: String,
    sha1: Option<String>,
    size: Option<u64>,
}

fn invalid(what: impl std::fmt::Display) -> LoaderInstallError {
    LoaderInstallError::Invalid(what.to_string())
}

/// Parses an install profile. Any `spec` other than 1 (older Forge profiles
/// have none) is `UnsupportedLoader`.
pub(super) fn parse_install_profile(
    kind: LoaderKind,
    bytes: &[u8],
) -> Result<InstallProfile, LoaderInstallError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| invalid(format!("install_profile.json: {error}")))?;
    if value.get("spec").and_then(Value::as_u64) != Some(SUPPORTED_SPEC) {
        return Err(LoaderInstallError::UnsupportedLoader(kind));
    }
    let raw = RawProfile::deserialize(value)
        .map_err(|error| invalid(format!("install_profile.json: {error}")))?;
    let json = RelativePath::parse(raw.json.trim_start_matches('/'))
        .map_err(|error| invalid(format!("install_profile.json json: {error}")))?;
    let data = raw
        .data
        .into_iter()
        .filter_map(|(key, value)| value.client.map(|client| (key, client)))
        .collect();
    let processors = raw
        .processors
        .into_iter()
        .map(processor)
        .collect::<Result<_, _>>()?;
    let libraries = raw
        .libraries
        .into_iter()
        .map(|library| artifact(library, true))
        .collect::<Result<_, _>>()?;
    Ok(InstallProfile {
        version: raw.version,
        minecraft: raw.minecraft,
        json,
        data,
        processors,
        libraries,
    })
}

fn coordinate(raw: &str) -> Result<MavenCoordinate, LoaderInstallError> {
    MavenCoordinate::parse(raw).map_err(|error| invalid(format!("processor library: {error}")))
}

fn processor(raw: RawProcessor) -> Result<ProcessorSpec, LoaderInstallError> {
    Ok(ProcessorSpec {
        jar: coordinate(&raw.jar)?,
        classpath: raw
            .classpath
            .iter()
            .map(String::as_str)
            .map(coordinate)
            .collect::<Result<_, _>>()?,
        args: raw.args,
        outputs: raw.outputs.into_iter().collect(),
        sides: raw.sides,
    })
}

/// A library's download. `require_sha1` refuses one without a sha1; a
/// malformed sha1 is always refused.
fn artifact(raw: RawLibrary, require_sha1: bool) -> Result<Artifact, LoaderInstallError> {
    let name = raw.name;
    let coordinate = MavenCoordinate::parse(&name)
        .map_err(|error| invalid(format!("library {name}: {error}")))?;
    let download = raw
        .downloads
        .and_then(|downloads| downloads.artifact)
        .ok_or_else(|| invalid(format!("library {name} declares no download")))?;
    let path = match download.path.as_deref() {
        Some(path) => RelativePath::parse(path)
            .map_err(|error| invalid(format!("library {name}: {error}")))?,
        None => coordinate.path(),
    };
    let checksum = match download.sha1.as_deref() {
        Some(sha1) => Some(
            Checksum::parse(HashAlgorithm::Sha1, sha1)
                .map_err(|error| invalid(format!("library {name}: bad sha1 ({error})")))?,
        ),
        None if require_sha1 => return Err(invalid(format!("library {name} has no sha1"))),
        None => None,
    };
    Ok(Artifact {
        path,
        url: download.url,
        checksum,
        size: download.size,
    })
}

/// The `version.json` libraries without a URL: the installer ships them in
/// `maven/` or a processor produces them.
pub(super) fn local_libraries(version: &Value) -> Result<Vec<Artifact>, LoaderInstallError> {
    let Some(libraries) = version.get("libraries").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut local = Vec::new();
    for library in libraries {
        let without_url = library
            .pointer("/downloads/artifact/url")
            .and_then(Value::as_str)
            .is_some_and(str::is_empty);
        if without_url {
            let raw = RawLibrary::deserialize(library)
                .map_err(|error| invalid(format!("version.json library: {error}")))?;
            local.push(artifact(raw, false)?);
        }
    }
    Ok(local)
}
