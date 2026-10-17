//! Mojang JSON formats and their mapping into the launch domain model.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::source::{ASSET_OBJECTS_BASE_URL, LIBRARIES_BASE_URL};
use crate::domain::instance::GameVersion;
use crate::domain::integrity::{Checksum, HashAlgorithm};
use crate::domain::java::JavaRequirement;
use crate::domain::launch::{
    ArgumentToken, Artifact, AssetIndex, AssetIndexRef, AssetObject, LegacyNatives, Library,
    LoggingConfig, MavenCoordinate, OsCondition, RelativePath, Rule, RuleAction, VersionKind,
    VersionManifest, VersionSourceError, VersionSpec, VersionSummary,
};

// ---------------------------------------------------------------------------
// Version manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawManifest {
    latest: RawLatest,
    #[serde(default)]
    versions: Vec<RawManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct RawLatest {
    release: String,
    snapshot: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifestEntry {
    id: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    url: String,
    sha1: Option<String>,
    #[serde(default)]
    release_time: String,
}

/// One downloadable entry of the version manifest, before validation.
#[derive(Debug, Clone)]
pub(super) struct ManifestEntry {
    pub id: String,
    pub url: String,
    pub sha1: Option<String>,
}

fn invalid(reason: impl Into<String>) -> VersionSourceError {
    VersionSourceError::Invalid(reason.into())
}

fn from_json<T: DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T, VersionSourceError> {
    serde_json::from_slice(bytes).map_err(|error| invalid(format!("{what}: {error}")))
}

fn manifest_kind(raw: &str) -> Option<VersionKind> {
    match raw {
        "release" => Some(VersionKind::Release),
        "snapshot" => Some(VersionKind::Snapshot),
        "old_beta" => Some(VersionKind::OldBeta),
        "old_alpha" => Some(VersionKind::OldAlpha),
        _ => None,
    }
}

pub(super) fn parse_manifest(bytes: &[u8]) -> Result<VersionManifest, VersionSourceError> {
    let raw: RawManifest = from_json(bytes, "version manifest")?;
    let latest = |id: &str| {
        GameVersion::parse(id).map_err(|_| invalid(format!("latest version id {id:?} is unsafe")))
    };
    let versions = raw
        .versions
        .into_iter()
        .filter_map(|entry| {
            let (Ok(id), Some(kind)) = (GameVersion::parse(&entry.id), manifest_kind(&entry.kind))
            else {
                tracing::warn!(id = %entry.id, kind = %entry.kind, "skipping a manifest entry");
                return None;
            };
            Some(VersionSummary {
                id,
                kind,
                release_time: entry.release_time,
            })
        })
        .collect();
    Ok(VersionManifest {
        latest_release: latest(&raw.latest.release)?,
        latest_snapshot: latest(&raw.latest.snapshot)?,
        versions,
    })
}

pub(super) fn manifest_entries(bytes: &[u8]) -> Result<Vec<ManifestEntry>, VersionSourceError> {
    let raw: RawManifest = from_json(bytes, "version manifest")?;
    Ok(raw
        .versions
        .into_iter()
        .map(|entry| ManifestEntry {
            id: entry.id,
            url: entry.url,
            sha1: entry.sha1,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Version JSON
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawVersion {
    id: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    main_class: Option<String>,
    arguments: Option<RawArguments>,
    minecraft_arguments: Option<String>,
    #[serde(default)]
    libraries: Vec<RawLibrary>,
    downloads: Option<RawVersionDownloads>,
    asset_index: Option<RawAssetIndex>,
    java_version: Option<RawJavaVersion>,
    logging: Option<RawLogging>,
    inherits_from: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawArguments {
    #[serde(default)]
    game: Vec<RawArgument>,
    #[serde(default)]
    jvm: Vec<RawArgument>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawArgument {
    Plain(String),
    Conditional {
        #[serde(default)]
        rules: Vec<RawRule>,
        value: RawValue,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct RawRule {
    action: RawAction,
    os: Option<RawOs>,
    #[serde(default)]
    features: BTreeMap<String, bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawAction {
    Allow,
    Disallow,
}

#[derive(Debug, Deserialize)]
struct RawOs {
    name: Option<String>,
    arch: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawLibrary {
    name: String,
    downloads: Option<RawLibraryDownloads>,
    /// Maven repository base (Fabric/Quilt profiles).
    url: Option<String>,
    sha1: Option<String>,
    size: Option<u64>,
    natives: Option<BTreeMap<String, String>>,
    extract: Option<RawExtract>,
    #[serde(default)]
    rules: Vec<RawRule>,
}

#[derive(Debug, Deserialize)]
struct RawLibraryDownloads {
    artifact: Option<RawDownload>,
    #[serde(default)]
    classifiers: BTreeMap<String, RawDownload>,
}

#[derive(Debug, Deserialize)]
struct RawDownload {
    path: Option<String>,
    #[serde(default)]
    url: String,
    sha1: Option<String>,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawExtract {
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawVersionDownloads {
    client: Option<RawDownload>,
}

#[derive(Debug, Deserialize)]
struct RawAssetIndex {
    id: String,
    sha1: Option<String>,
    size: Option<u64>,
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawJavaVersion {
    component: String,
    major_version: u32,
}

#[derive(Debug, Deserialize)]
struct RawLogging {
    client: Option<RawLoggingClient>,
}

#[derive(Debug, Deserialize)]
struct RawLoggingClient {
    argument: String,
    file: RawLoggingFile,
}

#[derive(Debug, Deserialize)]
struct RawLoggingFile {
    id: String,
    sha1: Option<String>,
    size: Option<u64>,
    url: String,
}

fn sha1(raw: Option<&str>, what: &str) -> Result<Option<Checksum>, VersionSourceError> {
    raw.map(|raw| {
        Checksum::parse(HashAlgorithm::Sha1, raw)
            .map_err(|error| invalid(format!("{what}: bad sha1 ({error})")))
    })
    .transpose()
}

fn relative_path(raw: &str, what: &str) -> Result<RelativePath, VersionSourceError> {
    RelativePath::parse(raw).map_err(|error| invalid(format!("{what}: {error}")))
}

fn artifact(
    path: RelativePath,
    url: String,
    raw_sha1: Option<&str>,
    size: Option<u64>,
    what: &str,
) -> Result<Artifact, VersionSourceError> {
    Ok(Artifact {
        checksum: sha1(raw_sha1, what)?,
        path,
        url,
        size,
    })
}

fn map_rules(rules: Vec<RawRule>) -> Vec<Rule> {
    rules
        .into_iter()
        .map(|rule| Rule {
            action: match rule.action {
                RawAction::Allow => RuleAction::Allow,
                RawAction::Disallow => RuleAction::Disallow,
            },
            os: rule.os.map(|os| OsCondition {
                name: os.name,
                arch: os.arch,
                version: os.version,
            }),
            features: rule.features,
        })
        .collect()
}

fn map_arguments(arguments: Vec<RawArgument>) -> Vec<ArgumentToken> {
    arguments
        .into_iter()
        .map(|argument| match argument {
            RawArgument::Plain(value) => ArgumentToken::Plain(value),
            RawArgument::Conditional { rules, value } => ArgumentToken::Conditional {
                rules: map_rules(rules),
                values: match value {
                    RawValue::One(value) => vec![value],
                    RawValue::Many(values) => values,
                },
            },
        })
        .collect()
}

/// A `downloads` entry; without a `path`, the coordinate's repository path.
fn download(
    raw: RawDownload,
    default_path: impl FnOnce() -> Result<RelativePath, VersionSourceError>,
    what: &str,
) -> Result<Artifact, VersionSourceError> {
    let path = match raw.path.as_deref() {
        Some(path) => relative_path(path, what)?,
        None => default_path()?,
    };
    artifact(path, raw.url, raw.sha1.as_deref(), raw.size, what)
}

fn map_library(raw: RawLibrary) -> Result<Library, VersionSourceError> {
    let name =
        MavenCoordinate::parse(&raw.name).map_err(|error| invalid(format!("library: {error}")))?;
    let what = format!("library {}", raw.name);
    let (artifact_download, classifiers) = match raw.downloads {
        Some(downloads) => (downloads.artifact, downloads.classifiers),
        None => (None, BTreeMap::new()),
    };

    let jar = match artifact_download {
        Some(raw_artifact) => Some(download(raw_artifact, || Ok(name.path()), &what)?),
        None if raw.natives.is_some() => None,
        None => {
            let base = raw.url.as_deref().unwrap_or(LIBRARIES_BASE_URL);
            let separator = if base.ends_with('/') { "" } else { "/" };
            let path = name.path();
            Some(artifact(
                path.clone(),
                format!("{base}{separator}{path}"),
                raw.sha1.as_deref(),
                raw.size,
                &what,
            )?)
        }
    };

    let natives = match raw.natives {
        Some(natives) => {
            let mut artifacts = BTreeMap::new();
            for (classifier, raw_download) in classifiers {
                // Re-parsing validates the classifier before it becomes a path.
                let default_path = || {
                    let classified = MavenCoordinate {
                        classifier: Some(classifier.clone()),
                        ..name.clone()
                    };
                    MavenCoordinate::parse(&classified.to_string())
                        .map(|coordinate| coordinate.path())
                        .map_err(|error| invalid(format!("{what}: {error}")))
                };
                let native = download(raw_download, default_path, &what)?;
                artifacts.insert(classifier, native);
            }
            Some(LegacyNatives {
                classifiers: natives,
                artifacts,
                exclude: raw
                    .extract
                    .map(|extract| extract.exclude)
                    .unwrap_or_default(),
            })
        }
        None => None,
    };

    Ok(Library {
        name,
        artifact: jar,
        natives,
        rules: map_rules(raw.rules),
    })
}

fn version_kind(raw: Option<&str>) -> VersionKind {
    raw.and_then(manifest_kind).unwrap_or(VersionKind::Release)
}

pub(super) fn parse_version(bytes: &[u8]) -> Result<VersionSpec, VersionSourceError> {
    let raw: RawVersion = from_json(bytes, "version JSON")?;
    let id = raw.id;

    let client_jar = match raw.downloads.and_then(|downloads| downloads.client) {
        Some(client) => {
            let path = relative_path(&format!("{id}/{id}.jar"), "client jar")?;
            Some(artifact(
                path,
                client.url,
                client.sha1.as_deref(),
                client.size,
                "client jar",
            )?)
        }
        None => None,
    };

    let asset_index = match raw.asset_index {
        Some(index) => {
            let path = AssetIndex::index_path(&index.id)
                .ok_or_else(|| invalid(format!("asset index id {:?} is unsafe", index.id)))?;
            Some(AssetIndexRef {
                artifact: artifact(
                    path,
                    index.url,
                    index.sha1.as_deref(),
                    index.size,
                    "asset index",
                )?,
                id: index.id,
            })
        }
        None => None,
    };

    let logging = match raw.logging.and_then(|logging| logging.client) {
        Some(client) => {
            let file = client.file;
            let path = relative_path(&format!("log_configs/{}", file.id), "logging config")?;
            Some(LoggingConfig {
                argument: client.argument,
                file: artifact(
                    path,
                    file.url,
                    file.sha1.as_deref(),
                    file.size,
                    "logging config",
                )?,
            })
        }
        None => None,
    };

    let (game_arguments, jvm_arguments) = match raw.arguments {
        Some(arguments) => (map_arguments(arguments.game), map_arguments(arguments.jvm)),
        None => (vec![], vec![]),
    };

    Ok(VersionSpec {
        kind: version_kind(raw.kind.as_deref()),
        main_class: raw.main_class.unwrap_or_default(),
        game_arguments,
        jvm_arguments,
        legacy_game_arguments: raw
            .minecraft_arguments
            .map(|args| args.split_whitespace().map(str::to_owned).collect()),
        libraries: raw
            .libraries
            .into_iter()
            .map(map_library)
            .collect::<Result<_, _>>()?,
        client_jar,
        asset_index,
        java: raw.java_version.map(|java| JavaRequirement {
            component: java.component,
            major: java.major_version,
        }),
        logging,
        inherits_from: raw.inherits_from,
        id,
    })
}

// ---------------------------------------------------------------------------
// Asset index
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawAssetIndexFile {
    #[serde(default)]
    objects: BTreeMap<String, RawAssetObject>,
}

#[derive(Debug, Deserialize)]
struct RawAssetObject {
    hash: String,
    size: u64,
}

pub(super) fn parse_asset_index(id: &str, bytes: &[u8]) -> Result<AssetIndex, VersionSourceError> {
    let raw: RawAssetIndexFile = from_json(bytes, "asset index")?;
    let objects = raw
        .objects
        .into_iter()
        .map(|(name, object)| {
            let hash = Checksum::parse(HashAlgorithm::Sha1, &object.hash)
                .map_err(|error| invalid(format!("asset {name}: bad hash ({error})")))?;
            let digest = hash.value();
            let url = format!(
                "{ASSET_OBJECTS_BASE_URL}/{}/{digest}",
                digest.get(..2).unwrap_or(digest)
            );
            Ok(AssetObject {
                name,
                hash,
                size: object.size,
                url,
            })
        })
        .collect::<Result<_, VersionSourceError>>()?;
    Ok(AssetIndex {
        id: id.to_owned(),
        objects,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::*;
    use crate::domain::instance::{JvmArgs, MemorySettings};
    use crate::domain::launch::{
        Arch, ArgumentToken, LaunchContext, LaunchIdentity, OsName, Platform, VersionKind,
        build_launch_command, plan_libraries,
    };

    const MANIFEST: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mojang/version_manifest_v2.json"
    ));
    const V26_2: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mojang/version-26.2.json"
    ));
    const V1_12_2: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mojang/version-1.12.2.json"
    ));
    const ASSETS_32: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mojang/asset-index-32-trimmed.json"
    ));

    fn context(platform: Platform, quick_play: Option<&str>) -> LaunchContext {
        LaunchContext {
            platform,
            identity: LaunchIdentity {
                player_name: "polispol1".into(),
                uuid: Uuid::parse_str("613bd4fe-6f7f-3488-8542-a216abf295e0").unwrap(),
                access_token: "secret-token".into(),
                user_type: "legacy",
                xuid: "0".into(),
            },
            game_dir: PathBuf::from("/data/instances/cinepol/minecraft"),
            assets_root: PathBuf::from("/data/assets"),
            natives_dir: PathBuf::from("/data/instances/cinepol/natives"),
            libraries_dir: PathBuf::from("/data/libraries"),
            classpath: vec![PathBuf::from("/data/versions/26.2/26.2.jar")],
            launcher_name: "cinepol-launcher".into(),
            launcher_version: "0.1.0".into(),
            memory: MemorySettings::new(1024, 4096).unwrap(),
            extra_jvm_args: JvmArgs::parse(vec![]).unwrap(),
            resolution: None,
            quick_play_multiplayer: quick_play.map(str::to_owned),
            logging_config: Some(PathBuf::from("/data/assets/log_configs/client-1.21.2.xml")),
        }
    }

    fn conditional_values(tokens: &[ArgumentToken]) -> Vec<String> {
        tokens
            .iter()
            .filter_map(|token| match token {
                ArgumentToken::Conditional { values, .. } => Some(values.clone()),
                ArgumentToken::Plain(_) => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn maps_the_version_manifest() {
        let manifest = parse_manifest(MANIFEST).unwrap();
        assert_eq!(manifest.latest_release.as_str(), "26.3");
        assert_eq!(manifest.latest_snapshot.as_str(), "26.4-snapshot-4");
        assert_eq!(manifest.versions.len(), 7);
        let kind_of = |id: &str| {
            manifest
                .versions
                .iter()
                .find(|version| version.id.as_str() == id)
                .map(|version| version.kind)
        };
        assert_eq!(kind_of("26.4-snapshot-4"), Some(VersionKind::Snapshot));
        assert_eq!(kind_of("26.2"), Some(VersionKind::Release));
        assert_eq!(kind_of("b1.7.3"), Some(VersionKind::OldBeta));
        assert_eq!(
            manifest.versions[2].release_time,
            "2026-06-16T12:03:33+00:00"
        );
    }

    #[test]
    fn skips_manifest_entries_with_unsafe_ids_or_unknown_types() {
        let raw = br#"{
            "latest": {"release": "1.0", "snapshot": "1.0"},
            "versions": [
                {"id": "1.0", "type": "release", "url": "https://x/1.0.json", "releaseTime": "t"},
                {"id": "../evil", "type": "release", "url": "https://x/e.json"},
                {"id": "2.0", "type": "experiment", "url": "https://x/2.0.json"},
                {"id": "a0.1", "type": "old_alpha", "url": "https://x/a.json", "extra": 1}
            ]
        }"#;
        let manifest = parse_manifest(raw).unwrap();
        let ids: Vec<_> = manifest.versions.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["1.0", "a0.1"]);
        assert_eq!(manifest.versions[1].kind, VersionKind::OldAlpha);
    }

    #[test]
    fn lists_manifest_download_entries() {
        let entries = manifest_entries(MANIFEST).unwrap();
        let entry = entries.iter().find(|entry| entry.id == "26.2").unwrap();
        assert_eq!(
            entry.url,
            "https://piston-meta.mojang.com/v1/packages/91ef51b7d36ca092c5c02aba42f0f40c7cc1db84/26.2.json"
        );
        assert_eq!(
            entry.sha1.as_deref(),
            Some("91ef51b7d36ca092c5c02aba42f0f40c7cc1db84")
        );
    }

    #[test]
    fn maps_the_modern_version_json() {
        let spec = parse_version(V26_2).unwrap();
        assert_eq!(spec.id, "26.2");
        assert_eq!(spec.kind, VersionKind::Release);
        assert_eq!(spec.main_class, "net.minecraft.client.main.Main");
        let java = spec.java.clone().unwrap();
        assert_eq!(java.component, "java-runtime-epsilon");
        assert_eq!(java.major, 25);
        assert_eq!(spec.libraries.len(), 131);
        assert_eq!(spec.legacy_game_arguments, None);
        assert_eq!(spec.inherits_from, None);

        let start_on_first_thread = spec.jvm_arguments.iter().find_map(|token| match token {
            ArgumentToken::Conditional { rules, values }
                if values == &["-XstartOnFirstThread".to_owned()] =>
            {
                Some(rules.clone())
            }
            _ => None,
        });
        let rules = start_on_first_thread.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].os.as_ref().and_then(|os| os.name.as_deref()),
            Some("osx")
        );
        assert!(spec.jvm_arguments.contains(&ArgumentToken::Plain(
            "-Djava.library.path=${natives_directory}/java".into()
        )));
        assert!(
            conditional_values(&spec.game_arguments).contains(&"--quickPlayMultiplayer".into())
        );
        assert!(
            spec.game_arguments
                .contains(&ArgumentToken::Plain("${auth_player_name}".into()))
        );

        let logging = spec.logging.clone().unwrap();
        assert_eq!(logging.argument, "-Dlog4j.configurationFile=${path}");
        assert_eq!(logging.file.path.as_str(), "log_configs/client-1.21.2.xml");
        assert_eq!(logging.file.size, Some(1073));

        let client = spec.client_jar.clone().unwrap();
        assert_eq!(client.path.as_str(), "26.2/26.2.jar");
        assert_eq!(
            client.checksum.map(|c| c.value().to_owned()),
            Some("2dc72797acbc1b63fc16a11c4ac393605f453754".into())
        );

        let index = spec.asset_index.clone().unwrap();
        assert_eq!(index.id, "32");
        assert_eq!(index.artifact.path.as_str(), "indexes/32.json");
        assert_eq!(index.artifact.size, Some(586_366));

        let lz4 = &spec.libraries[0];
        assert_eq!(lz4.name.to_string(), "at.yawk.lz4:lz4-java:1.10.1");
        let artifact = lz4.artifact.clone().unwrap();
        assert_eq!(
            artifact.path.as_str(),
            "at/yawk/lz4/lz4-java/1.10.1/lz4-java-1.10.1.jar"
        );
        assert_eq!(artifact.size, Some(910_232));
    }

    #[test]
    fn plans_only_the_host_os_natives_for_the_modern_version() {
        let spec = parse_version(V26_2).unwrap();
        let cases = [
            (Platform::new(OsName::MacOs, Arch::Arm64), "natives-macos"),
            (
                Platform::new(OsName::Windows, Arch::X86_64),
                "natives-windows",
            ),
            (Platform::new(OsName::Linux, Arch::Arm64), "natives-linux"),
        ];
        for (platform, marker) in cases {
            let plan = plan_libraries(&spec, &platform, &Default::default());
            assert!(!plan.classpath.is_empty());
            let natives: Vec<_> = plan
                .classpath
                .iter()
                .map(|artifact| artifact.path.as_str())
                .filter(|path| path.contains("natives-"))
                .collect();
            assert!(!natives.is_empty(), "{marker}");
            assert!(
                natives.iter().all(|path| path.contains(marker)),
                "{marker}: {natives:?}"
            );
            assert!(plan.natives.is_empty());
        }
    }

    #[test]
    fn builds_a_macos_command_with_quick_play() {
        let spec = parse_version(V26_2).unwrap();
        let platform = Platform::new(OsName::MacOs, Arch::Arm64);

        let command = build_launch_command(&spec, &context(platform.clone(), None));
        assert!(
            command
                .jvm_args
                .contains(&"-XstartOnFirstThread".to_owned())
        );
        assert!(command.jvm_args.contains(
            &"-Dlog4j.configurationFile=/data/assets/log_configs/client-1.21.2.xml".to_owned()
        ));
        assert!(
            !command
                .game_args
                .contains(&"--quickPlayMultiplayer".to_owned())
        );

        let command = build_launch_command(&spec, &context(platform, Some("mc.policarpiers.com")));
        assert_eq!(
            command.game_args[command.game_args.len() - 2..],
            ["--quickPlayMultiplayer", "mc.policarpiers.com"]
        );
    }

    #[test]
    fn maps_the_legacy_version_json() {
        let spec = parse_version(V1_12_2).unwrap();
        let legacy = spec.legacy_game_arguments.clone().unwrap();
        assert_eq!(legacy[..2], ["--username", "${auth_player_name}"]);
        assert_eq!(legacy.len(), 18);
        assert!(spec.jvm_arguments.is_empty());
        assert!(spec.game_arguments.is_empty());
        assert_eq!(spec.java.as_ref().map(|java| java.major), Some(8));

        let jinput = spec
            .libraries
            .iter()
            .find(|library| library.name.artifact == "jinput-platform")
            .unwrap();
        assert_eq!(jinput.artifact, None, "natives-only entries have no jar");
        let natives = jinput.natives.as_ref().unwrap();
        assert_eq!(
            natives.classifiers.get("osx").map(String::as_str),
            Some("natives-osx")
        );
        assert_eq!(natives.artifacts.len(), 3);
        assert_eq!(natives.exclude, ["META-INF/"]);
    }

    #[test]
    fn picks_legacy_natives_per_os() {
        let spec = parse_version(V1_12_2).unwrap();
        let native_paths = |platform: Platform| -> Vec<String> {
            plan_libraries(&spec, &platform, &Default::default())
                .natives
                .iter()
                .map(|native| {
                    assert_eq!(native.exclude, ["META-INF/"]);
                    native.artifact.path.to_string()
                })
                .collect()
        };

        let windows = native_paths(Platform::new(OsName::Windows, Arch::X86_64));
        assert!(windows.contains(
            &"org/lwjgl/lwjgl/lwjgl-platform/2.9.4-nightly-20150209/lwjgl-platform-2.9.4-nightly-20150209-natives-windows.jar".to_owned()
        ));
        assert!(
            windows
                .iter()
                .all(|path| path.ends_with("natives-windows.jar"))
        );

        let macos = native_paths(Platform::new(OsName::MacOs, Arch::X86_64));
        assert!(macos.contains(
            &"org/lwjgl/lwjgl/lwjgl-platform/2.9.2-nightly-20140822/lwjgl-platform-2.9.2-nightly-20140822-natives-osx.jar".to_owned()
        ));
        assert!(macos.iter().all(|path| path.ends_with("natives-osx.jar")));
    }

    #[test]
    fn maps_maven_style_libraries_without_downloads() {
        let raw = br#"{
            "id": "fabric-loader-0.17.2-26.2",
            "inheritsFrom": "26.2",
            "type": "release",
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "arguments": {"game": [], "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "]},
            "libraries": [
                {"name": "net.fabricmc:sponge-mixin:0.16.4+mixin.0.8.7", "url": "https://maven.fabricmc.net",
                 "sha1": "AB0C2E4B5C3E7D1F2A9B8C7D6E5F4A3B2C1D0E9F", "size": 1500},
                {"name": "org.ow2.asm:asm:9.8"}
            ]
        }"#;
        let spec = parse_version(raw).unwrap();
        assert_eq!(spec.inherits_from.as_deref(), Some("26.2"));
        assert_eq!(spec.client_jar, None);
        assert_eq!(spec.java, None);
        let mixin = spec.libraries[0].artifact.clone().unwrap();
        assert_eq!(
            mixin.url,
            "https://maven.fabricmc.net/net/fabricmc/sponge-mixin/0.16.4+mixin.0.8.7/sponge-mixin-0.16.4+mixin.0.8.7.jar"
        );
        assert_eq!(
            mixin.checksum.map(|c| c.value().to_owned()),
            Some("ab0c2e4b5c3e7d1f2a9b8c7d6e5f4a3b2c1d0e9f".into())
        );
        assert_eq!(mixin.size, Some(1500));
        let asm = spec.libraries[1].artifact.clone().unwrap();
        assert_eq!(
            asm.url,
            "https://libraries.minecraft.net/org/ow2/asm/asm/9.8/asm-9.8.jar"
        );
        assert_eq!(asm.checksum, None);
    }

    #[test]
    fn rejects_unsafe_names_paths_and_digests() {
        let cases: [&[u8]; 5] = [
            br#"{"id": "x", "libraries": [{"name": "a:b:1:../../x"}]}"#,
            br#"{"id": "x", "libraries": [{"name": "a:b:1", "downloads": {"artifact": {"path": "../a.jar", "url": "u"}}}]}"#,
            br#"{"id": "x", "libraries": [{"name": "a:b:1", "sha1": "nothex"}]}"#,
            br#"{"id": "x", "logging": {"client": {"argument": "a", "file": {"id": "../x.xml", "url": "u"}}}}"#,
            br#"not json"#,
        ];
        for raw in cases {
            assert!(
                matches!(parse_version(raw), Err(VersionSourceError::Invalid(_))),
                "{}",
                String::from_utf8_lossy(raw)
            );
        }
    }

    #[test]
    fn maps_rule_conditions_and_unknown_types() {
        let raw = br#"{
            "id": "x",
            "type": "pending",
            "arguments": {"game": [
                {"rules": [{"action": "disallow", "os": {"name": "windows", "arch": "x86", "version": "^10\\."},
                            "features": {"has_custom_resolution": true}}],
                 "value": "--flag"}
            ]}
        }"#;
        let spec = parse_version(raw).unwrap();
        assert_eq!(spec.kind, VersionKind::Release);
        assert_eq!(spec.main_class, "");
        let ArgumentToken::Conditional { rules, values } = &spec.game_arguments[0] else {
            panic!("expected a conditional argument");
        };
        assert_eq!(values, &["--flag"]);
        let rule = &rules[0];
        assert_eq!(rule.action, crate::domain::launch::RuleAction::Disallow);
        let os = rule.os.clone().unwrap();
        assert_eq!(os.name.as_deref(), Some("windows"));
        assert_eq!(os.arch.as_deref(), Some("x86"));
        assert_eq!(os.version.as_deref(), Some("^10\\."));
        assert_eq!(rule.features.get("has_custom_resolution"), Some(&true));
    }

    #[test]
    fn maps_asset_objects_to_content_addressed_urls() {
        let index = parse_asset_index("32", ASSETS_32).unwrap();
        assert_eq!(index.id, "32");
        assert_eq!(index.objects.len(), 6);
        let icon = index
            .objects
            .iter()
            .find(|object| object.name == "icons/icon_16x16.png")
            .unwrap();
        assert_eq!(icon.size, 781);
        assert_eq!(
            icon.url,
            "https://resources.download.minecraft.net/5f/5ff04807c356f1beed0b86ccf659b44b9983e3fa"
        );
        assert_eq!(
            icon.object_path().as_str(),
            "objects/5f/5ff04807c356f1beed0b86ccf659b44b9983e3fa"
        );
        assert!(matches!(
            parse_asset_index("32", br#"{"objects": {"a": {"hash": "../..", "size": 1}}}"#),
            Err(VersionSourceError::Invalid(_))
        ));
    }
}
