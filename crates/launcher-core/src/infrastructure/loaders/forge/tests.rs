use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use sha1::{Digest, Sha1};
use tokio_util::sync::CancellationToken;
use zip::write::SimpleFileOptions;

use super::manifest::main_class;
use super::profile::parse_install_profile;
use super::versions::{parse_forge_releases, parse_neoforge_releases};
use crate::domain::instance::GameVersion;
use crate::domain::integrity::HashAlgorithm;
use crate::domain::launch::LauncherPaths;
use crate::domain::loader::{
    LoaderEnvironment, LoaderInstallError, LoaderInstaller, LoaderKind, LoaderMetadataError,
    LoaderProfile, LoaderSpec, LoaderVersion, ProcessorCommand, ProcessorRunError, ProcessorRunner,
};
use crate::domain::ports::{HttpClient, HttpError, ProgressUpdate};
use crate::infrastructure::loaders::LoaderInstallers;
use crate::testing::{FakeHttpClient, RecordingReporter, forward_slashes, forward_slashes_all};

const FORGE_PROFILE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/forge/install_profile.json"
));
const FORGE_VERSION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/forge/version.json"
));
const FORGE_METADATA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/forge/maven-metadata-trimmed.xml"
));
const FORGE_PROMOTIONS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/forge/promotions_slim-trimmed.json"
));
const NEOFORGE_PROFILE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/neoforge/install_profile.json"
));
const NEOFORGE_VERSION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/neoforge/version.json"
));
const NEOFORGE_VERSIONS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/neoforge/versions-trimmed.json"
));

const FORGE_INSTALLER_URL: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge/26.2-65.1.5/forge-26.2-65.1.5-installer.jar";
const NEOFORGE_INSTALLER_URL: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge/26.2.0.93/neoforge-26.2.0.93-installer.jar";
const FORGE_ID: &str = "26.2-forge-65.1.5";
const NEOFORGE_ID: &str = "neoforge-26.2.0.93";
const BINARYPATCHER_MAIN: &str = "net.minecraftforge.binarypatcher.ConsoleTool";
const INSTALLERTOOLS_MAIN: &str = "net.neoforged.installertools.ConsoleTool";
const PATCHED: &[u8] = b"the patched client";

// ---------------------------------------------------------------- helpers

fn game() -> GameVersion {
    GameVersion::parse("26.2").unwrap()
}

fn spec(kind: LoaderKind, version: &str) -> LoaderSpec {
    LoaderSpec::new(kind, Some(LoaderVersion::parse(version).unwrap())).unwrap()
}

fn forge() -> LoaderSpec {
    spec(LoaderKind::Forge, "65.1.5")
}

fn neoforge() -> LoaderSpec {
    spec(LoaderKind::NeoForge, "26.2.0.93")
}

fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

/// The real Forge profile, with `PATCHED_SHA` set to what the fake runner writes.
fn forge_profile() -> Value {
    let mut profile = json(FORGE_PROFILE);
    profile["data"]["PATCHED_SHA"]["client"] = Value::String(format!("'{}'", sha1_hex(PATCHED)));
    profile
}

/// A small installer with the real JSONs and fake `data/` and `maven/` files.
fn build_installer(profile: &Value, version: &[u8], extra: &[(&str, &[u8])]) -> Vec<u8> {
    let profile = bytes(profile);
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("install_profile.json", &profile),
        ("version.json", version),
        ("data/client.lzma", b"fake client patches"),
        ("data/server.lzma", b"fake server patches"),
        (
            "maven/net/minecraftforge/forge/26.2-65.1.5/forge-26.2-65.1.5-shim.jar",
            b"fake shim",
        ),
    ];
    entries.extend_from_slice(extra);
    zip(&entries)
}

fn forge_installer() -> Vec<u8> {
    build_installer(&forge_profile(), FORGE_VERSION, &[])
}

fn neoforge_installer() -> Vec<u8> {
    build_installer(&json(NEOFORGE_PROFILE), NEOFORGE_VERSION, &[])
}

/// Serves an installer and its `.sha1` sidecar.
fn online(url: &str, installer: &[u8]) -> FakeHttpClient {
    FakeHttpClient::with([
        (url.to_owned(), installer.to_vec()),
        (format!("{url}.sha1"), sha1_hex(installer).into_bytes()),
    ])
}

#[derive(Debug)]
struct Offline;

impl HttpClient for Offline {
    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        Err(HttpError::Network {
            url: url.to_owned(),
            reason: "offline".into(),
        })
    }
}

/// Records every command; writes the configured files, or waits for cancel.
#[derive(Debug, Clone, Default)]
struct FakeRunner {
    commands: Arc<Mutex<Vec<ProcessorCommand>>>,
    writes: Vec<(PathBuf, Vec<u8>)>,
    wait_for_cancel: bool,
    failure: Option<String>,
    /// Produces nothing, like a tool that exits 0 without writing its output.
    silent: bool,
}

impl FakeRunner {
    fn writing(path: PathBuf, body: &[u8]) -> Self {
        Self {
            writes: vec![(path, body.to_vec())],
            ..Self::default()
        }
    }

    fn commands(&self) -> Vec<ProcessorCommand> {
        self.commands.lock().unwrap().clone()
    }
}

impl ProcessorRunner for FakeRunner {
    async fn run(
        &self,
        command: &ProcessorCommand,
        cancel: &CancellationToken,
    ) -> Result<(), ProcessorRunError> {
        self.commands.lock().unwrap().push(command.clone());
        if self.wait_for_cancel {
            cancel.cancelled().await;
            return Err(ProcessorRunError::Cancelled);
        }
        if let Some(output) = &self.failure {
            return Err(ProcessorRunError::Failed {
                code: Some(1),
                output: output.clone(),
            });
        }
        if self.silent {
            return Ok(());
        }
        // Like the real tools: the file after `--output` is produced...
        if let Some(pair) = command.args.windows(2).find(|pair| pair[0] == "--output") {
            let path = PathBuf::from(&pair[1]);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"produced").unwrap();
        }
        // ...unless the test scripts other contents for it.
        for (path, body) in &self.writes {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        Ok(())
    }
}

fn installers<H>(http: H, root: &Path, runner: FakeRunner) -> LoaderInstallers<H, FakeRunner> {
    LoaderInstallers::new(http, LauncherPaths::new(root), runner)
}

fn libraries(root: &Path) -> PathBuf {
    LauncherPaths::new(root).libraries_dir()
}

fn patched_path(root: &Path) -> PathBuf {
    libraries(root).join("net/minecraftforge/forge/26.2-65.1.5/forge-26.2-65.1.5-client.jar")
}

/// Puts every tool in place, as the use case does; processor jars get a manifest.
fn place_tools(root: &Path, profile: &LoaderProfile) {
    for tool in &profile.tools {
        let path = tool.path.under(&libraries(root));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let main = if tool.path.as_str().contains("binarypatcher") {
            Some(BINARYPATCHER_MAIN)
        } else if tool.path.as_str().contains("installertools") {
            Some(INSTALLERTOOLS_MAIN)
        } else {
            None
        };
        let manifest = main
            .map(|main| format!("Manifest-Version: 1.0\r\nMain-Class: {main}\r\n\r\n"))
            .unwrap_or_else(|| "Manifest-Version: 1.0\r\n\r\n".to_owned());
        std::fs::write(&path, zip(&[("META-INF/MANIFEST.MF", manifest.as_bytes())])).unwrap();
    }
}

struct Installed {
    dir: tempfile::TempDir,
    client_jar: PathBuf,
    java: PathBuf,
}

impl Installed {
    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn environment(&self) -> LoaderEnvironment<'_> {
        LoaderEnvironment {
            java: &self.java,
            client_jar: &self.client_jar,
        }
    }
}

/// Installs a loader into a fresh data root, with its tools and the vanilla jar in place.
async fn installed(loader: &LoaderSpec, url: &str, installer: &[u8]) -> Installed {
    let dir = tempfile::tempdir().unwrap();
    let profile = installers(online(url, installer), dir.path(), FakeRunner::default())
        .install(loader, &game())
        .await
        .unwrap();
    place_tools(dir.path(), &profile);
    let client_jar = dir.path().join("versions/26.2/26.2.jar");
    std::fs::create_dir_all(client_jar.parent().unwrap()).unwrap();
    std::fs::write(&client_jar, b"vanilla").unwrap();
    Installed {
        java: dir.path().join("runtime/bin/java"),
        dir,
        client_jar,
    }
}

async fn finish(
    installed: &Installed,
    loader: &LoaderSpec,
    runner: &FakeRunner,
) -> Result<(), LoaderInstallError> {
    installers(Offline, installed.root(), runner.clone())
        .finish(
            loader,
            &game(),
            installed.environment(),
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
}

// ---------------------------------------------------------------- fixtures

#[test]
fn every_install_profile_fixture_parses() {
    let forge = parse_install_profile(LoaderKind::Forge, FORGE_PROFILE).unwrap();
    assert_eq!(forge.version, FORGE_ID);
    assert_eq!(forge.minecraft, "26.2");
    assert_eq!(forge.json.as_str(), "version.json");
    assert_eq!(forge.processors.len(), 5);
    assert_eq!(forge.libraries.len(), 16);
    assert_eq!(forge.data["BINPATCH"], "/data/client.lzma");
    assert_eq!(
        forge.data["PATCHED"],
        "[net.minecraftforge:forge:26.2-65.1.5:client]"
    );
    let client: Vec<_> = forge
        .processors
        .iter()
        .filter(|p| {
            p.sides
                .as_ref()
                .is_some_and(|s| s.contains(&"client".to_owned()))
        })
        .collect();
    assert_eq!(client.len(), 1);
    assert_eq!(
        client[0].jar.to_string(),
        "net.minecraftforge:binarypatcher:1.3.4"
    );
    assert_eq!(
        client[0].outputs,
        [("{PATCHED}".to_owned(), "{PATCHED_SHA}".to_owned())]
    );

    let neoforge = parse_install_profile(LoaderKind::NeoForge, NEOFORGE_PROFILE).unwrap();
    assert_eq!(neoforge.version, NEOFORGE_ID);
    assert_eq!(neoforge.processors.len(), 2);
    assert_eq!(neoforge.libraries.len(), 29);
    assert_eq!(neoforge.processors[1].sides, None);
    assert!(neoforge.processors[1].outputs.is_empty());
    assert_eq!(neoforge.data["MCP_VERSION"], "'26.2-2'");

    for library in forge.libraries.iter().chain(&neoforge.libraries) {
        let checksum = library.checksum.as_ref().unwrap();
        assert_eq!(checksum.algorithm(), HashAlgorithm::Sha1);
        assert!(!library.url.is_empty());
    }
}

#[test]
fn every_version_fixture_is_the_profile_it_installs() {
    for (bytes, id) in [(FORGE_VERSION, FORGE_ID), (NEOFORGE_VERSION, NEOFORGE_ID)] {
        let version = json(bytes);
        assert_eq!(version["id"], id);
        assert_eq!(version["inheritsFrom"], "26.2");
    }
}

#[test]
fn only_spec_1_install_profiles_are_supported() {
    let mut profile = json(FORGE_PROFILE);
    profile["spec"] = Value::from(0);
    assert!(matches!(
        parse_install_profile(LoaderKind::Forge, &bytes(&profile)),
        Err(LoaderInstallError::UnsupportedLoader(LoaderKind::Forge))
    ));
    // Forge 1.12 and older: no spec, an `install` block instead.
    let legacy = serde_json::json!({"install": {"profileName": "forge"}, "versionInfo": {}});
    assert!(matches!(
        parse_install_profile(LoaderKind::Forge, &bytes(&legacy)),
        Err(LoaderInstallError::UnsupportedLoader(LoaderKind::Forge))
    ));
}

#[test]
fn a_profile_library_without_a_sha1_is_invalid() {
    let mut profile = json(FORGE_PROFILE);
    profile["libraries"][0]["downloads"]["artifact"]
        .as_object_mut()
        .unwrap()
        .remove("sha1");
    assert!(matches!(
        parse_install_profile(LoaderKind::Forge, &bytes(&profile)),
        Err(LoaderInstallError::Invalid(_))
    ));
}

// ---------------------------------------------------------------- manifest

#[test]
fn reads_the_main_class_of_the_main_section_with_continuation_lines() {
    let manifest = "Manifest-Version: 1.0\r\nMain-Class: net.minecraftforge.binarypatch\r\n er.ConsoleTool\r\nClass-Path: a.jar\r\n\r\nName: x/\r\nMain-Class: wrong.Main\r\n";
    assert_eq!(main_class(manifest).as_deref(), Some(BINARYPATCHER_MAIN));
    assert_eq!(
        main_class("Manifest-Version: 1.0\nmain-class:  a.B$C \n").as_deref(),
        Some("a.B$C")
    );
    assert_eq!(
        main_class("Manifest-Version: 1.0\n\nMain-Class: a.B\n"),
        None
    );
    assert_eq!(main_class("Main-Class: -jar\n"), None, "never a JVM option");
    assert_eq!(main_class("Main-Class: a b\n"), None);
    assert_eq!(main_class("Main-Class:\n"), None);
}

// ---------------------------------------------------------------- metadata

fn versions(releases: &[crate::domain::loader::LoaderRelease]) -> Vec<(&str, bool)> {
    releases
        .iter()
        .map(|release| (release.version.as_str(), release.stable))
        .collect()
}

#[test]
fn forge_releases_are_per_game_newest_first_and_stable_up_to_the_recommended_one() {
    let releases = parse_forge_releases(&game(), FORGE_METADATA, FORGE_PROMOTIONS).unwrap();
    assert_eq!(releases.len(), 16);
    assert!(
        releases
            .iter()
            .all(|release| release.game_version == game())
    );
    let listed = versions(&releases);
    assert_eq!(listed[0], ("65.1.5", false));
    assert!(listed.contains(&("65.1.1", false)));
    assert!(listed.contains(&("65.1.0", true)));
    assert!(listed.contains(&("65.0.9", true)));
    assert_eq!(listed[15], ("65.0.0", true));

    let older = GameVersion::parse("1.21.11").unwrap();
    assert_eq!(
        versions(&parse_forge_releases(&older, FORGE_METADATA, FORGE_PROMOTIONS).unwrap()),
        [
            ("61.2.4", false),
            ("61.2.3", false),
            ("61.2.1", false),
            ("61.2.0", true)
        ]
    );
}

#[test]
fn forge_releases_without_a_recommended_promotion_are_all_stable() {
    let next = GameVersion::parse("26.3").unwrap();
    assert_eq!(
        versions(&parse_forge_releases(&next, FORGE_METADATA, FORGE_PROMOTIONS).unwrap()),
        [
            ("66.0.13", true),
            ("66.0.12", true),
            ("66.0.11", true),
            ("66.0.10", true)
        ]
    );
    let unknown = GameVersion::parse("1.0").unwrap();
    assert_eq!(
        parse_forge_releases(&unknown, FORGE_METADATA, FORGE_PROMOTIONS),
        Err(LoaderMetadataError::UnsupportedGameVersion(
            LoaderKind::Forge,
            "1.0".into()
        ))
    );
}

#[test]
fn neoforge_releases_match_the_game_and_betas_are_not_stable() {
    let releases = parse_neoforge_releases(&game(), NEOFORGE_VERSIONS).unwrap();
    assert_eq!(releases.len(), 92);
    assert_eq!(releases.iter().filter(|release| release.stable).count(), 35);
    let listed = versions(&releases);
    assert_eq!(listed[0], ("26.2.0.93", true));
    assert!(listed.contains(&("26.2.0.56-beta", false)));
    assert!(listed.contains(&("26.2.0.57", true)));

    let legacy = GameVersion::parse("1.21.11").unwrap();
    assert_eq!(
        versions(&parse_neoforge_releases(&legacy, NEOFORGE_VERSIONS).unwrap()),
        [("21.11.48", true), ("21.11.41-beta", false)]
    );
    // 26.1.2.x belongs to Minecraft 26.1.2; snapshot builds never match a release.
    let hotfix = GameVersion::parse("26.1.2").unwrap();
    assert_eq!(
        versions(&parse_neoforge_releases(&hotfix, NEOFORGE_VERSIONS).unwrap()),
        [("26.1.2.122", true)]
    );
    let first = GameVersion::parse("26.1").unwrap();
    assert!(matches!(
        parse_neoforge_releases(&first, NEOFORGE_VERSIONS),
        Err(LoaderMetadataError::UnsupportedGameVersion(
            LoaderKind::NeoForge,
            _
        ))
    ));
}

#[tokio::test]
async fn metadata_reads_the_real_endpoints() {
    let forge = super::ForgeLoaderMetadata::forge(FakeHttpClient::with([
        (
            super::FORGE_METADATA_URL.to_owned(),
            FORGE_METADATA.to_vec(),
        ),
        (
            super::FORGE_PROMOTIONS_URL.to_owned(),
            FORGE_PROMOTIONS.to_vec(),
        ),
    ]));
    use crate::domain::loader::LoaderMetadata;
    assert_eq!(forge.kind(), LoaderKind::Forge);
    assert_eq!(forge.releases(&game()).await.unwrap().len(), 16);

    let neoforge = super::ForgeLoaderMetadata::neoforge(FakeHttpClient::with([(
        super::NEOFORGE_VERSIONS_URL.to_owned(),
        NEOFORGE_VERSIONS.to_vec(),
    )]));
    assert_eq!(neoforge.kind(), LoaderKind::NeoForge);
    assert_eq!(neoforge.releases(&game()).await.unwrap().len(), 92);

    assert!(matches!(
        super::ForgeLoaderMetadata::forge(Offline)
            .releases(&game())
            .await,
        Err(LoaderMetadataError::Network(_))
    ));
}

// ---------------------------------------------------------------- install

#[tokio::test]
async fn install_verifies_the_installer_and_stores_its_version_json() {
    let dir = tempfile::tempdir().unwrap();
    let installer = forge_installer();
    let http = online(FORGE_INSTALLER_URL, &installer);
    let loaders = installers(http, dir.path(), FakeRunner::default());
    let profile = loaders.install(&forge(), &game()).await.unwrap();

    assert_eq!(profile.id.as_str(), FORGE_ID);
    let stored = LauncherPaths::new(dir.path())
        .version_json(FORGE_ID)
        .unwrap();
    assert_eq!(std::fs::read(stored).unwrap(), FORGE_VERSION);
    assert_eq!(
        loaders.http.requests(),
        [
            format!("{FORGE_INSTALLER_URL}.sha1"),
            FORGE_INSTALLER_URL.to_owned()
        ]
    );
    // Kept for `finish`, under a launcher-owned path.
    let kept = libraries(dir.path())
        .join("net/minecraftforge/forge/26.2-65.1.5/forge-26.2-65.1.5-installer.jar");
    assert_eq!(std::fs::read(kept).unwrap(), installer);
    // The scratch extraction is gone.
    assert!(
        !LauncherPaths::new(dir.path())
            .temp_dir()
            .join("loaders")
            .join(FORGE_ID)
            .exists()
    );
}

#[tokio::test]
async fn install_returns_the_profile_libraries_as_checksummed_tools() {
    let dir = tempfile::tempdir().unwrap();
    let profile = installers(
        online(FORGE_INSTALLER_URL, &forge_installer()),
        dir.path(),
        FakeRunner::default(),
    )
    .install(&forge(), &game())
    .await
    .unwrap();
    let expected = json(FORGE_PROFILE);
    let expected = expected["libraries"].as_array().unwrap();
    assert_eq!(profile.tools.len(), expected.len());
    for (tool, library) in profile.tools.iter().zip(expected) {
        let artifact = &library["downloads"]["artifact"];
        assert_eq!(tool.path.as_str(), artifact["path"]);
        assert_eq!(tool.url, artifact["url"]);
        assert_eq!(tool.checksum.as_ref().unwrap().value(), artifact["sha1"]);
        assert_eq!(tool.size, artifact["size"].as_u64());
    }

    let dir = tempfile::tempdir().unwrap();
    let profile = installers(
        online(NEOFORGE_INSTALLER_URL, &neoforge_installer()),
        dir.path(),
        FakeRunner::default(),
    )
    .install(&neoforge(), &game())
    .await
    .unwrap();
    assert_eq!(profile.id.as_str(), NEOFORGE_ID);
    assert_eq!(profile.tools.len(), 29);
    assert!(profile.tools.iter().all(|tool| tool.checksum.is_some()));
    let stored = LauncherPaths::new(dir.path())
        .version_json(NEOFORGE_ID)
        .unwrap();
    assert_eq!(std::fs::read(stored).unwrap(), NEOFORGE_VERSION);
}

#[tokio::test]
async fn an_installer_that_does_not_match_its_sidecar_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let http = FakeHttpClient::with([
        (FORGE_INSTALLER_URL.to_owned(), forge_installer()),
        (
            format!("{FORGE_INSTALLER_URL}.sha1"),
            sha1_hex(b"another file").into_bytes(),
        ),
    ]);
    let result = installers(http, dir.path(), FakeRunner::default())
        .install(&forge(), &game())
        .await;
    assert!(
        matches!(result, Err(LoaderInstallError::Invalid(_))),
        "{result:?}"
    );
    assert!(!dir.path().join("versions").exists());
    assert!(!dir.path().join("libraries").exists());
}

#[tokio::test]
async fn reinstalling_reuses_the_verified_installer_and_works_offline() {
    let dir = tempfile::tempdir().unwrap();
    let installer = forge_installer();
    let first = installers(
        online(FORGE_INSTALLER_URL, &installer),
        dir.path(),
        FakeRunner::default(),
    )
    .install(&forge(), &game())
    .await
    .unwrap();

    let again = installers(
        online(FORGE_INSTALLER_URL, &installer),
        dir.path(),
        FakeRunner::default(),
    );
    assert_eq!(again.install(&forge(), &game()).await.unwrap(), first);
    assert_eq!(
        again.http.requests(),
        [format!("{FORGE_INSTALLER_URL}.sha1")],
        "the stored installer still matches"
    );

    let offline = installers(Offline, dir.path(), FakeRunner::default());
    assert_eq!(offline.install(&forge(), &game()).await.unwrap(), first);

    // Offline with a damaged installer: nothing to trust.
    let kept = libraries(dir.path())
        .join("net/minecraftforge/forge/26.2-65.1.5/forge-26.2-65.1.5-installer.jar");
    std::fs::write(&kept, b"damaged").unwrap();
    assert!(matches!(
        offline.install(&forge(), &game()).await,
        Err(LoaderInstallError::Network(_))
    ));
}

#[tokio::test]
async fn offline_without_an_installer_is_a_network_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        installers(Offline, dir.path(), FakeRunner::default())
            .install(&forge(), &game())
            .await,
        Err(LoaderInstallError::Network(_))
    ));
}

#[tokio::test]
async fn install_extracts_local_libraries_from_the_installer_maven_folder() {
    let mut profile = forge_profile();
    let shim = profile["libraries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|library| library["name"] == "net.minecraftforge:forge:26.2-65.1.5:shim")
        .unwrap();
    shim["downloads"]["artifact"]["url"] = Value::String(String::new());
    shim["downloads"]["artifact"]["sha1"] = Value::String(sha1_hex(b"fake shim"));
    let installer = build_installer(&profile, FORGE_VERSION, &[]);

    let dir = tempfile::tempdir().unwrap();
    let tools = installers(
        online(FORGE_INSTALLER_URL, &installer),
        dir.path(),
        FakeRunner::default(),
    )
    .install(&forge(), &game())
    .await
    .unwrap()
    .tools;
    let shim_path = libraries(dir.path())
        .join("net/minecraftforge/forge/26.2-65.1.5/forge-26.2-65.1.5-shim.jar");
    assert_eq!(std::fs::read(&shim_path).unwrap(), b"fake shim");
    assert!(tools.iter().any(|tool| tool.url.is_empty()));

    // A local library that does not match its sha1 is refused.
    let shim = profile["libraries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|library| library["name"] == "net.minecraftforge:forge:26.2-65.1.5:shim")
        .unwrap();
    shim["downloads"]["artifact"]["sha1"] = Value::String(sha1_hex(b"something else"));
    let installer = installer_bytes(&profile);
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        installers(
            online(FORGE_INSTALLER_URL, &installer),
            dir.path(),
            FakeRunner::default()
        )
        .install(&forge(), &game())
        .await,
        Err(LoaderInstallError::Invalid(_))
    ));
}

fn installer_bytes(profile: &Value) -> Vec<u8> {
    build_installer(profile, FORGE_VERSION, &[])
}

#[tokio::test]
async fn install_refuses_zip_slip_entries() {
    let installer = build_installer(
        &forge_profile(),
        FORGE_VERSION,
        &[("../../escaped.txt", b"x")],
    );
    let dir = tempfile::tempdir().unwrap();
    let result = installers(
        online(FORGE_INSTALLER_URL, &installer),
        dir.path(),
        FakeRunner::default(),
    )
    .install(&forge(), &game())
    .await;
    assert!(
        matches!(result, Err(LoaderInstallError::Invalid(_))),
        "{result:?}"
    );
    let temp = LauncherPaths::new(dir.path()).temp_dir();
    assert!(!temp.join("loaders").join("escaped.txt").exists());
    assert!(!temp.join("escaped.txt").exists());
    assert!(!dir.path().join("versions").exists());
}

#[tokio::test]
async fn install_refuses_unsupported_specs_and_mismatched_versions() {
    let mut profile = forge_profile();
    profile["spec"] = Value::from(2);
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        installers(
            online(FORGE_INSTALLER_URL, &installer_bytes(&profile)),
            dir.path(),
            FakeRunner::default()
        )
        .install(&forge(), &game())
        .await,
        Err(LoaderInstallError::UnsupportedLoader(LoaderKind::Forge))
    );

    let mut version = json(FORGE_VERSION);
    version["inheritsFrom"] = Value::String("26.1".into());
    let installer = build_installer(&forge_profile(), &bytes(&version), &[]);
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        installers(
            online(FORGE_INSTALLER_URL, &installer),
            dir.path(),
            FakeRunner::default()
        )
        .install(&forge(), &game())
        .await,
        Err(LoaderInstallError::Invalid(_))
    ));
    assert!(!dir.path().join("versions").exists());

    let mut version = json(FORGE_VERSION);
    version["id"] = Value::String("26.2-forge-65.1.4".into());
    let installer = build_installer(&forge_profile(), &bytes(&version), &[]);
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        installers(
            online(FORGE_INSTALLER_URL, &installer),
            dir.path(),
            FakeRunner::default()
        )
        .install(&forge(), &game())
        .await,
        Err(LoaderInstallError::Invalid(_))
    ));
}

// ---------------------------------------------------------------- finish

#[tokio::test]
async fn forge_finish_runs_only_the_client_binarypatcher_and_verifies_its_output() {
    let installed = installed(&forge(), FORGE_INSTALLER_URL, &forge_installer()).await;
    let root = installed.root();
    let runner = FakeRunner::writing(patched_path(root), PATCHED);
    let reporter = RecordingReporter::default();
    installers(Offline, root, runner.clone())
        .finish(
            &forge(),
            &game(),
            installed.environment(),
            &reporter,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    let commands = runner.commands();
    assert_eq!(commands.len(), 1, "{commands:#?}");
    let command = &commands[0];
    let libs = libraries(root);
    assert_eq!(command.java, installed.java);
    assert_eq!(command.main_class, BINARYPATCHER_MAIN);
    assert_eq!(command.working_dir, root);
    assert_eq!(
        command.classpath[0],
        libs.join("net/minecraftforge/binarypatcher/1.3.4/binarypatcher-1.3.4.jar")
    );
    assert_eq!(command.classpath.len(), 7);
    assert_eq!(
        command.classpath[6],
        libs.join("trove/trove/1.0.2/trove-1.0.2.jar")
    );
    let binpatch = PathBuf::from(&command.args[5]);
    assert!(binpatch.starts_with(LauncherPaths::new(root).temp_dir()));
    assert!(binpatch.ends_with("data/client.lzma"));
    assert_eq!(
        forward_slashes_all(&command.args),
        forward_slashes_all(&[
            "--clean",
            installed.client_jar.to_str().unwrap(),
            "--output",
            patched_path(root).to_str().unwrap(),
            "--apply",
            binpatch.to_str().unwrap(),
            "--data",
            "--unpatched",
            "--store",
            "--marker",
            ".forge_patched_minecraft",
        ])
    );
    assert_eq!(std::fs::read(patched_path(root)).unwrap(), PATCHED);
    assert_eq!(
        reporter.updates().last(),
        Some(&ProgressUpdate::new(1, Some(1)))
    );
    // The extracted installer files are cleaned up.
    assert!(
        !LauncherPaths::new(root)
            .temp_dir()
            .join("loaders")
            .join(FORGE_ID)
            .exists()
    );
}

#[tokio::test]
async fn a_wrong_output_is_a_mismatch_and_is_removed() {
    let installed = installed(&forge(), FORGE_INSTALLER_URL, &forge_installer()).await;
    let runner = FakeRunner::writing(patched_path(installed.root()), b"not what was declared");
    assert!(matches!(
        finish(&installed, &forge(), &runner).await,
        Err(LoaderInstallError::OutputMismatch(_))
    ));
    assert!(!patched_path(installed.root()).exists());
}

#[tokio::test]
async fn a_missing_output_or_a_failed_tool_is_a_processor_failure() {
    let installed = installed(&forge(), FORGE_INSTALLER_URL, &forge_installer()).await;
    let silent = FakeRunner {
        silent: true,
        ..FakeRunner::default()
    };
    assert!(matches!(
        finish(&installed, &forge(), &silent).await,
        Err(LoaderInstallError::ProcessorFailed(_))
    ));
    let failing = FakeRunner {
        failure: Some("Exception in thread main".into()),
        ..FakeRunner::default()
    };
    let result = finish(&installed, &forge(), &failing).await;
    assert!(
        matches!(&result, Err(LoaderInstallError::ProcessorFailed(reason)) if reason.contains("Exception in thread main")),
        "{result:?}"
    );
}

#[tokio::test]
async fn finishing_again_skips_verified_outputs() {
    let installed = installed(&forge(), FORGE_INSTALLER_URL, &forge_installer()).await;
    let runner = FakeRunner::writing(patched_path(installed.root()), PATCHED);
    finish(&installed, &forge(), &runner).await.unwrap();
    finish(&installed, &forge(), &runner).await.unwrap();
    assert_eq!(runner.commands().len(), 1);

    // A damaged output runs the processor again.
    std::fs::write(patched_path(installed.root()), b"damaged").unwrap();
    finish(&installed, &forge(), &runner).await.unwrap();
    assert_eq!(runner.commands().len(), 2);
    assert_eq!(
        std::fs::read(patched_path(installed.root())).unwrap(),
        PATCHED
    );
}

#[tokio::test]
async fn neoforge_finish_runs_its_processor_once_per_plan() {
    let installed = installed(&neoforge(), NEOFORGE_INSTALLER_URL, &neoforge_installer()).await;
    let root = installed.root();
    let runner = FakeRunner::default();
    finish(&installed, &neoforge(), &runner).await.unwrap();

    let commands = runner.commands();
    assert_eq!(commands.len(), 1, "only the client processor");
    let command = &commands[0];
    assert_eq!(command.main_class, INSTALLERTOOLS_MAIN);
    let libs = libraries(root);
    let tools = libs.join(
        "net/neoforged/installertools/installertools/4.0.17/installertools-4.0.17-fatjar.jar",
    );
    assert_eq!(command.classpath, [tools.clone(), tools]);
    let root_libraries = format!("{}/libraries/", root.display());
    assert_eq!(
        command.args[..5],
        [
            "--task",
            "PROCESS_MINECRAFT_JAR",
            "--no-mod-manifest",
            "--input",
            installed.client_jar.to_str().unwrap(),
        ]
    );
    assert_eq!(
        forward_slashes(&command.args[6]),
        forward_slashes(
            libs.join("net/neoforged/minecraft-client-patched/26.2.0.93/minecraft-client-patched-26.2.0.93.jar")
                .to_str()
                .unwrap()
        )
    );
    assert_eq!(
        forward_slashes(&command.args[8]),
        forward_slashes(&root_libraries)
    );

    // The marker skips the second run.
    finish(&installed, &neoforge(), &runner).await.unwrap();
    assert_eq!(runner.commands().len(), 1);

    // A different plan (another client jar) runs again.
    let moved = root.join("versions/26.2/moved.jar");
    std::fs::copy(&installed.client_jar, &moved).unwrap();
    installers(Offline, root, runner.clone())
        .finish(
            &neoforge(),
            &game(),
            LoaderEnvironment {
                java: &installed.java,
                client_jar: &moved,
            },
            &RecordingReporter::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(runner.commands().len(), 2);

    // A deleted patched jar is rebuilt even though the plan did not change.
    let patched = libraries(root).join(
        "net/neoforged/minecraft-client-patched/26.2.0.93/minecraft-client-patched-26.2.0.93.jar",
    );
    std::fs::remove_file(&patched).unwrap();
    finish(&installed, &neoforge(), &runner).await.unwrap();
    assert_eq!(runner.commands().len(), 3);
    assert!(patched.is_file());
}

#[tokio::test]
async fn a_failed_neoforge_run_leaves_no_marker() {
    let installed = installed(&neoforge(), NEOFORGE_INSTALLER_URL, &neoforge_installer()).await;
    let failing = FakeRunner {
        failure: Some("boom".into()),
        ..FakeRunner::default()
    };
    assert!(finish(&installed, &neoforge(), &failing).await.is_err());
    let runner = FakeRunner::default();
    finish(&installed, &neoforge(), &runner).await.unwrap();
    assert_eq!(runner.commands().len(), 1);
}

#[tokio::test]
async fn cancelling_stops_the_processors() {
    let installed = installed(&forge(), FORGE_INSTALLER_URL, &forge_installer()).await;
    let runner = FakeRunner {
        wait_for_cancel: true,
        ..FakeRunner::default()
    };
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.cancel();
    });
    let result = installers(Offline, installed.root(), runner.clone())
        .finish(
            &forge(),
            &game(),
            installed.environment(),
            &RecordingReporter::default(),
            &cancel,
        )
        .await;
    assert_eq!(result, Err(LoaderInstallError::Cancelled));
    assert_eq!(runner.commands().len(), 1);

    // Already cancelled: nothing runs.
    let runner = FakeRunner::default();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let result = installers(Offline, installed.root(), runner.clone())
        .finish(
            &forge(),
            &game(),
            installed.environment(),
            &RecordingReporter::default(),
            &cancelled,
        )
        .await;
    assert_eq!(result, Err(LoaderInstallError::Cancelled));
    assert!(runner.commands().is_empty());
}

#[tokio::test]
async fn finish_re_verifies_the_stored_installer() {
    let installed = installed(&forge(), FORGE_INSTALLER_URL, &forge_installer()).await;
    let kept = libraries(installed.root())
        .join("net/minecraftforge/forge/26.2-65.1.5/forge-26.2-65.1.5-installer.jar");
    std::fs::write(&kept, b"tampered").unwrap();
    let runner = FakeRunner::writing(patched_path(installed.root()), PATCHED);
    assert!(matches!(
        finish(&installed, &forge(), &runner).await,
        Err(LoaderInstallError::Invalid(_))
    ));
    assert!(runner.commands().is_empty());
}

#[tokio::test]
async fn a_processor_jar_without_a_main_class_fails() {
    let installed = installed(&forge(), FORGE_INSTALLER_URL, &forge_installer()).await;
    let jar = libraries(installed.root())
        .join("net/minecraftforge/binarypatcher/1.3.4/binarypatcher-1.3.4.jar");
    std::fs::write(
        &jar,
        zip(&[("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n")]),
    )
    .unwrap();
    let runner = FakeRunner::writing(patched_path(installed.root()), PATCHED);
    assert!(matches!(
        finish(&installed, &forge(), &runner).await,
        Err(LoaderInstallError::ProcessorFailed(_))
    ));
    assert!(runner.commands().is_empty());
}
