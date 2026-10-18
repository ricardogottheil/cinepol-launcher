use std::path::{Path, PathBuf};

use serde_json::Value;

use super::meta::{parse_releases, parse_sidecar};
use super::*;
use crate::domain::launch::VersionSource;
use crate::domain::loader::{LoaderMetadata, LoaderMetadataError, LoaderVersion};
use crate::domain::ports::HttpError;
use crate::infrastructure::mojang::{MojangVersionSource, VERSION_MANIFEST_URL};
use crate::testing::FakeHttpClient;

const FABRIC_RELEASES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/fabric/loader-versions-26.2-trimmed.json"
));
const FABRIC_PROFILE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/fabric/profile-0.20.0-26.2.json"
));
const FABRIC_LOADER_SIDECAR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/fabric/fabric-loader-0.20.0.jar.sha1"
));
const QUILT_RELEASES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/quilt/loader-versions-26.2-trimmed.json"
));
const QUILT_PROFILE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/quilt/profile-0.20.0-beta.9-26.2.json"
));
/// One `<sidecar body> <sidecar url>` line per Quilt library.
const QUILT_SIDECARS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/quilt/sidecars-0.20.0-beta.9-26.2.txt"
));
const MOJANG_MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/mojang/version_manifest_v2.json"
));
const MOJANG_26_2: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/mojang/version-26.2.json"
));
const MOJANG_26_2_URL: &str =
    "https://piston-meta.mojang.com/v1/packages/91ef51b7d36ca092c5c02aba42f0f40c7cc1db84/26.2.json";

const FABRIC_RELEASES_URL: &str = "https://meta.fabricmc.net/v2/versions/loader/26.2";
const QUILT_RELEASES_URL: &str = "https://meta.quiltmc.org/v3/versions/loader/26.2";
const FABRIC_PROFILE_URL: &str =
    "https://meta.fabricmc.net/v2/versions/loader/26.2/0.20.0/profile/json";
const QUILT_PROFILE_URL: &str =
    "https://meta.quiltmc.org/v3/versions/loader/26.2/0.20.0-beta.9/profile/json";
const FABRIC_LOADER_SIDECAR_URL: &str =
    "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.20.0/fabric-loader-0.20.0.jar.sha1";
const FABRIC_ID: &str = "fabric-loader-0.20.0-26.2";
const QUILT_ID: &str = "quilt-loader-0.20.0-beta.9-26.2";

/// Every request fails as if the machine were offline.
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

/// Every request answers the same HTTP error status.
#[derive(Debug)]
struct Answers(u16);

impl HttpClient for Answers {
    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        Err(HttpError::Status {
            url: url.to_owned(),
            status: self.0,
        })
    }
}

fn game() -> GameVersion {
    GameVersion::parse("26.2").unwrap()
}

fn spec(kind: LoaderKind, version: &str) -> LoaderSpec {
    LoaderSpec::new(kind, Some(LoaderVersion::parse(version).unwrap())).unwrap()
}

fn fabric() -> LoaderSpec {
    spec(LoaderKind::Fabric, "0.20.0")
}

fn quilt() -> LoaderSpec {
    spec(LoaderKind::Quilt, "0.20.0-beta.9")
}

fn quilt_sidecars() -> Vec<(String, Vec<u8>)> {
    QUILT_SIDECARS
        .lines()
        .map(|line| {
            let (body, url) = line.split_once(' ').unwrap();
            (url.to_owned(), body.as_bytes().to_vec())
        })
        .collect()
}

fn fabric_online(profile: &[u8]) -> FakeHttpClient {
    FakeHttpClient::with([
        (FABRIC_PROFILE_URL.to_owned(), profile.to_vec()),
        (
            FABRIC_LOADER_SIDECAR_URL.to_owned(),
            FABRIC_LOADER_SIDECAR.to_vec(),
        ),
    ])
}

fn quilt_online(sidecars: Vec<(String, Vec<u8>)>) -> FakeHttpClient {
    let mut bodies = sidecars;
    bodies.push((QUILT_PROFILE_URL.to_owned(), QUILT_PROFILE.to_vec()));
    FakeHttpClient::with(bodies)
}

fn installers<H>(http: H, root: &Path) -> LoaderInstallers<H> {
    LoaderInstallers::new(http, LauncherPaths::new(root))
}

fn cached_path(root: &Path, id: &str) -> PathBuf {
    LauncherPaths::new(root).version_json(id).unwrap()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn libraries(profile: &Value) -> &Vec<Value> {
    profile["libraries"].as_array().unwrap()
}

fn sha1_of(library: &Value) -> Option<&str> {
    library.get("sha1").and_then(Value::as_str)
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    files.sort();
    files
}

fn with_field(profile: &[u8], field: &str, value: &str) -> Vec<u8> {
    let mut json: Value = serde_json::from_slice(profile).unwrap();
    json[field] = Value::String(value.to_owned());
    serde_json::to_vec(&json).unwrap()
}

#[test]
fn parses_every_fixture() {
    let fabric = parse_releases(LoaderKind::Fabric, &game(), FABRIC_RELEASES).unwrap();
    let versions: Vec<(&str, bool)> = fabric
        .iter()
        .map(|release| (release.version.as_str(), release.stable))
        .collect();
    assert_eq!(
        versions,
        [
            ("0.20.0", false),
            ("0.19.5", true),
            ("0.19.4", false),
            ("0.19.3", false)
        ]
    );
    assert!(fabric.iter().all(|release| release.game_version == game()));

    let quilt = parse_releases(LoaderKind::Quilt, &game(), QUILT_RELEASES).unwrap();
    let versions: Vec<(&str, bool)> = quilt
        .iter()
        .map(|release| (release.version.as_str(), release.stable))
        .collect();
    assert_eq!(
        versions,
        [
            ("0.20.0-beta.9", false),
            ("0.20.0-beta.7", false),
            ("0.20.0-beta.8", false)
        ]
    );

    for (profile, id, count) in [
        (FABRIC_PROFILE, FABRIC_ID, 7),
        (QUILT_PROFILE, QUILT_ID, 12),
    ] {
        let json: Value = serde_json::from_slice(profile).unwrap();
        assert_eq!(json["id"], id);
        assert_eq!(json["inheritsFrom"], "26.2");
        assert_eq!(libraries(&json).len(), count);
    }

    assert_eq!(
        parse_sidecar(FABRIC_LOADER_SIDECAR).as_deref(),
        Some("7a3a8ce24fa0150aba52f3e1f7cde5ba17195fcf")
    );
    let sidecars = quilt_sidecars();
    assert_eq!(sidecars.len(), 12);
    for (url, body) in sidecars {
        assert!(parse_sidecar(&body).is_some(), "{url}");
    }
}

#[test]
fn sidecars_may_carry_a_file_name_and_must_be_a_sha1() {
    assert_eq!(
        parse_sidecar(b"7A3A8CE24FA0150ABA52F3E1F7CDE5BA17195FCF  fabric-loader-0.20.0.jar\n")
            .as_deref(),
        Some("7a3a8ce24fa0150aba52f3e1f7cde5ba17195fcf")
    );
    for body in [
        &b""[..],
        b"   \n",
        b"<html>Not Found</html>",
        b"7a3a8ce24fa0150aba52f3e1f7cde5ba17195fc",
        b"7a3a8ce24fa0150aba52f3e1f7cde5ba17195fcz",
        b"\xff\xfe",
    ] {
        assert_eq!(parse_sidecar(body), None, "{body:?}");
    }
}

#[tokio::test]
async fn lists_releases_through_the_meta_apis() {
    let fabric = MetaLoaderMetadata::fabric(FakeHttpClient::with([(
        FABRIC_RELEASES_URL.to_owned(),
        FABRIC_RELEASES.to_vec(),
    )]));
    assert_eq!(fabric.kind(), LoaderKind::Fabric);
    let releases = fabric.releases(&game()).await.unwrap();
    assert_eq!(releases.len(), 4);
    assert_eq!(releases[1].version.as_str(), "0.19.5");

    let quilt = MetaLoaderMetadata::quilt(FakeHttpClient::with([(
        QUILT_RELEASES_URL.to_owned(),
        QUILT_RELEASES.to_vec(),
    )]));
    assert_eq!(quilt.kind(), LoaderKind::Quilt);
    let releases = quilt.releases(&game()).await.unwrap();
    assert_eq!(releases.len(), 3);
    assert_eq!(releases[0].version.as_str(), "0.20.0-beta.9");
}

#[tokio::test]
async fn unknown_game_versions_are_unsupported() {
    let unknown = GameVersion::parse("9.9.9").unwrap();
    let unsupported = |kind| {
        Err(LoaderMetadataError::UnsupportedGameVersion(
            kind,
            "9.9.9".into(),
        ))
    };

    // Fabric answers 400 with `[]`; Quilt answers 404.
    assert_eq!(
        MetaLoaderMetadata::fabric(Answers(400))
            .releases(&unknown)
            .await,
        unsupported(LoaderKind::Fabric)
    );
    assert_eq!(
        MetaLoaderMetadata::quilt(FakeHttpClient::default())
            .releases(&unknown)
            .await,
        unsupported(LoaderKind::Quilt)
    );
    let empty = FakeHttpClient::with([(
        "https://meta.fabricmc.net/v2/versions/loader/9.9.9".to_owned(),
        b"[]".to_vec(),
    )]);
    assert_eq!(
        MetaLoaderMetadata::fabric(empty).releases(&unknown).await,
        unsupported(LoaderKind::Fabric)
    );
    assert!(matches!(
        MetaLoaderMetadata::quilt(Offline).releases(&unknown).await,
        Err(LoaderMetadataError::Network(_))
    ));
    assert!(matches!(
        MetaLoaderMetadata::quilt(Answers(503))
            .releases(&unknown)
            .await,
        Err(LoaderMetadataError::Network(_))
    ));
}

#[tokio::test]
async fn fabric_install_fills_only_the_missing_sha1() {
    let dir = tempfile::tempdir().unwrap();
    let http = fabric_online(FABRIC_PROFILE);
    let installer = installers(http, dir.path());
    let id = installer.install(&fabric(), &game()).await.unwrap();
    assert_eq!(id.as_str(), FABRIC_ID);
    assert_eq!(
        installer.http.requests(),
        [FABRIC_PROFILE_URL, FABRIC_LOADER_SIDECAR_URL]
    );

    let cached = read_json(&cached_path(dir.path(), FABRIC_ID));
    let mut expected: Value = serde_json::from_slice(FABRIC_PROFILE).unwrap();
    expected["libraries"][6]["sha1"] = "7a3a8ce24fa0150aba52f3e1f7cde5ba17195fcf".into();
    assert_eq!(
        cached, expected,
        "only the missing sha1 is added; the rest of the profile is kept"
    );
}

#[tokio::test]
async fn quilt_install_fills_every_sha1_from_the_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let sidecars = quilt_sidecars();
    let installer = installers(quilt_online(sidecars.clone()), dir.path());
    let id = installer.install(&quilt(), &game()).await.unwrap();
    assert_eq!(id.as_str(), QUILT_ID);
    assert_eq!(installer.http.requests().len(), 13);

    let cached = read_json(&cached_path(dir.path(), QUILT_ID));
    let mut expected: Value = serde_json::from_slice(QUILT_PROFILE).unwrap();
    for (library, (url, body)) in expected["libraries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(&sidecars)
    {
        let name = library["name"].as_str().unwrap();
        let artifact = name.split(':').nth(1).unwrap();
        assert!(url.contains(artifact), "{url} belongs to {name}");
        library["sha1"] = std::str::from_utf8(body).unwrap().into();
    }
    assert_eq!(cached, expected);
    assert!(libraries(&cached).iter().all(|lib| sha1_of(lib).is_some()));
}

#[tokio::test]
async fn cached_profiles_resolve_through_the_version_source_with_checksums() {
    let dir = tempfile::tempdir().unwrap();
    installers(fabric_online(FABRIC_PROFILE), dir.path())
        .install(&fabric(), &game())
        .await
        .unwrap();
    installers(quilt_online(quilt_sidecars()), dir.path())
        .install(&quilt(), &game())
        .await
        .unwrap();

    let source = MojangVersionSource::new(
        FakeHttpClient::with([
            (VERSION_MANIFEST_URL.to_owned(), MOJANG_MANIFEST.to_vec()),
            (MOJANG_26_2_URL.to_owned(), MOJANG_26_2.to_vec()),
        ]),
        LauncherPaths::new(dir.path()),
    );
    for (id, main_class, count) in [
        (
            FABRIC_ID,
            "net.fabricmc.loader.impl.launch.knot.KnotClient",
            7,
        ),
        (
            QUILT_ID,
            "org.quiltmc.loader.impl.launch.knot.KnotClient",
            12,
        ),
    ] {
        let spec = source.version(id).await.unwrap();
        assert_eq!(spec.id, id);
        assert_eq!(spec.main_class, main_class);
        for library in &spec.libraries[..count] {
            let artifact = library.artifact.as_ref().unwrap();
            assert!(
                artifact.checksum.is_some(),
                "{} has no checksum",
                library.name
            );
            assert!(artifact.url.ends_with(".jar"), "{}", artifact.url);
        }
    }
}

#[tokio::test]
async fn a_malformed_or_missing_sidecar_fails_and_caches_nothing() {
    let mut malformed = quilt_sidecars();
    malformed[4].1 = b"<html>oops</html>".to_vec();
    let mut missing = quilt_sidecars();
    missing.remove(7);

    for sidecars in [malformed, missing] {
        let dir = tempfile::tempdir().unwrap();
        let result = installers(quilt_online(sidecars), dir.path())
            .install(&quilt(), &game())
            .await;
        assert!(
            matches!(result, Err(LoaderInstallError::Invalid(_))),
            "{result:?}"
        );
        assert!(files_under(dir.path()).is_empty());
    }
}

#[tokio::test]
async fn a_declared_but_malformed_sha1_is_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let mut json: Value = serde_json::from_slice(FABRIC_PROFILE).unwrap();
    json["libraries"][0]["sha1"] = "not-a-sha1".into();
    let result = installers(
        fabric_online(&serde_json::to_vec(&json).unwrap()),
        dir.path(),
    )
    .install(&fabric(), &game())
    .await;
    assert!(matches!(result, Err(LoaderInstallError::Invalid(_))));
    assert!(files_under(dir.path()).is_empty());
}

#[tokio::test]
async fn a_mismatched_profile_id_or_parent_is_invalid() {
    for profile in [
        with_field(FABRIC_PROFILE, "id", "fabric-loader-0.19.5-26.2"),
        with_field(FABRIC_PROFILE, "inheritsFrom", "26.1"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let result = installers(fabric_online(&profile), dir.path())
            .install(&fabric(), &game())
            .await;
        assert!(
            matches!(result, Err(LoaderInstallError::Invalid(_))),
            "{result:?}"
        );
        assert!(files_under(dir.path()).is_empty());
    }
}

#[tokio::test]
async fn unknown_loader_versions_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let expected = Err(LoaderInstallError::UnknownLoaderVersion(
        LoaderKind::Fabric,
        "0.20.0".into(),
        "26.2".into(),
    ));
    // Fabric answers 400 for an unknown loader or game version, Quilt 404.
    assert_eq!(
        installers(Answers(400), dir.path())
            .install(&fabric(), &game())
            .await,
        expected
    );
    assert_eq!(
        installers(FakeHttpClient::default(), dir.path())
            .install(&fabric(), &game())
            .await,
        expected
    );
}

#[tokio::test]
async fn offline_installs_keep_a_valid_cached_profile() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        installers(Offline, dir.path())
            .install(&quilt(), &game())
            .await,
        Err(LoaderInstallError::Network(_))
    ));

    installers(quilt_online(quilt_sidecars()), dir.path())
        .install(&quilt(), &game())
        .await
        .unwrap();
    let path = cached_path(dir.path(), QUILT_ID);
    let before = std::fs::read(&path).unwrap();
    let id = installers(Offline, dir.path())
        .install(&quilt(), &game())
        .await
        .unwrap();
    assert_eq!(id.as_str(), QUILT_ID);
    assert_eq!(std::fs::read(&path).unwrap(), before);

    // A cached profile with unhashed libraries is not launchable.
    std::fs::write(&path, QUILT_PROFILE).unwrap();
    assert!(matches!(
        installers(Answers(503), dir.path())
            .install(&quilt(), &game())
            .await,
        Err(LoaderInstallError::Network(_))
    ));
}

#[tokio::test]
async fn vanilla_needs_no_profile_and_forge_is_not_supported_yet() {
    let dir = tempfile::tempdir().unwrap();
    let installer = installers(FakeHttpClient::default(), dir.path());
    assert_eq!(
        installer
            .install(&LoaderSpec::vanilla(), &game())
            .await
            .unwrap()
            .as_str(),
        "26.2"
    );
    for (kind, version) in [
        (LoaderKind::Forge, "47.2.0"),
        (LoaderKind::NeoForge, "21.1.77"),
    ] {
        assert_eq!(
            installer.install(&spec(kind, version), &game()).await,
            Err(LoaderInstallError::UnsupportedLoader(kind))
        );
    }
    assert!(installer.http.requests().is_empty());
    assert!(files_under(dir.path()).is_empty());
}

#[tokio::test]
async fn writes_nothing_outside_the_profile_directory() {
    let dir = tempfile::tempdir().unwrap();
    installers(fabric_online(FABRIC_PROFILE), dir.path())
        .install(&fabric(), &game())
        .await
        .unwrap();
    installers(quilt_online(quilt_sidecars()), dir.path())
        .install(&quilt(), &game())
        .await
        .unwrap();
    assert_eq!(
        files_under(dir.path()),
        [
            Path::new("versions")
                .join(FABRIC_ID)
                .join(format!("{FABRIC_ID}.json")),
            Path::new("versions")
                .join(QUILT_ID)
                .join(format!("{QUILT_ID}.json")),
        ]
    );
}

#[tokio::test]
async fn spaced_game_versions_are_escaped_in_the_profile_url() {
    let dir = tempfile::tempdir().unwrap();
    let installer = installers(FakeHttpClient::default(), dir.path());
    // Old pre-release ids hold spaces; a profile id may too, the URL may not.
    let spaced = GameVersion::parse("1.14 Pre-Release 1").unwrap();
    assert!(matches!(
        installer.install(&fabric(), &spaced).await,
        Err(LoaderInstallError::UnknownLoaderVersion(..))
    ));
    let requests = installer.http.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].contains("1.14%20Pre-Release%201"),
        "{}",
        requests[0]
    );
    assert!(!requests[0].contains(' '));
}
