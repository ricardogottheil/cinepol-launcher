//! Installs the latest stable Fabric and Quilt profiles for 26.2 from the real
//! meta APIs and checks that every cached library declares a sha1.
//!
//! Opt-in only: `cargo test -p cinepol-launcher-core --features network-tests --test loaders_network`
#![cfg(feature = "network-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use launcher_core::domain::instance::GameVersion;
use launcher_core::domain::launch::LauncherPaths;
use launcher_core::domain::loader::{LoaderInstaller, LoaderMetadata, LoaderSpec};
use launcher_core::infrastructure::http::ReqwestHttpClient;
use launcher_core::infrastructure::loaders::{LoaderInstallers, MetaLoaderMetadata};
use serde_json::Value;

const USER_AGENT: &str =
    "ricardogottheil/cinepol-launcher/0.1.0 (https://cinepol.policarpiers.com)";

fn http() -> ReqwestHttpClient {
    ReqwestHttpClient::new(USER_AGENT).unwrap()
}

async fn install_latest_stable(metadata: impl LoaderMetadata, paths: &LauncherPaths) {
    let game = GameVersion::parse("26.2").unwrap();
    let releases = metadata.releases(&game).await.unwrap();
    let release = releases
        .iter()
        .find(|release| release.stable)
        .unwrap_or(&releases[0]);
    let loader = LoaderSpec::new(metadata.kind(), Some(release.version.clone())).unwrap();

    let id = LoaderInstallers::new(http(), paths.clone())
        .install(&loader, &game)
        .await
        .unwrap();
    let path = paths.version_json(id.as_str()).unwrap();
    let profile: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let libraries = profile["libraries"].as_array().unwrap();
    assert!(!libraries.is_empty());
    for library in libraries {
        let sha1 = library["sha1"].as_str().unwrap_or_default();
        assert_eq!(sha1.len(), 40, "{} has no sha1", library["name"]);
    }
    println!("{id}: {} libraries, all hashed", libraries.len());
}

#[tokio::test]
async fn installs_the_latest_stable_fabric_and_quilt_profiles_for_26_2() {
    let dir = tempfile::tempdir().unwrap();
    let paths = LauncherPaths::new(dir.path());
    install_latest_stable(MetaLoaderMetadata::fabric(http()), &paths).await;
    install_latest_stable(MetaLoaderMetadata::quilt(http()), &paths).await;
}
