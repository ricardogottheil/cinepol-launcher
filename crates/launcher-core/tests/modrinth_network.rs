//! Searches the real Modrinth API, lists the versions of Fabulously Optimized
//! (`1KVo5zza`) and looks up one file of its pack by sha512.
//!
//! Opt-in only: `cargo test -p cinepol-launcher-core --features network-tests --test modrinth_network`
#![cfg(feature = "network-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use launcher_core::domain::instance::GameVersion;
use launcher_core::domain::integrity::HashAlgorithm;
use launcher_core::domain::loader::LoaderKind;
use launcher_core::domain::modpack::{ModpackQuery, ModpackSource};
use launcher_core::infrastructure::http::ReqwestHttpClient;
use launcher_core::infrastructure::modrinth::ModrinthSource;

const USER_AGENT: &str =
    "ricardogottheil/cinepol-launcher/0.1.0 (https://cinepol.policarpiers.com)";

const FABULOUSLY_OPTIMIZED: &str = "1KVo5zza";

/// BetterGrassify 1.8.7 for Fabric 26.2, listed in the Fabulously Optimized 14.1.1 index.
const KNOWN_SHA512: &str = "7b70e796cea2ee57a6022108092517b6aa39d5a19b8da1de26685ae53b8fbb4717cd91ee1d232c54efa8e210ebe9b760d3dab79bf9a5fdb5ae75d97b461a1fab";

fn modrinth() -> ModrinthSource<ReqwestHttpClient> {
    ModrinthSource::new(ReqwestHttpClient::new(USER_AGENT).unwrap())
}

#[tokio::test]
async fn searches_lists_versions_and_finds_known_files() {
    let modrinth = modrinth();

    let page = modrinth
        .search(&ModpackQuery {
            text: "fabulously optimized".into(),
            game_version: Some(GameVersion::parse("26.2").unwrap()),
            loader: Some(LoaderKind::Fabric),
            page: 0,
            page_size: 5,
        })
        .await
        .unwrap();
    assert!(page.items.len() <= 5);
    assert!(
        page.items
            .iter()
            .any(|item| item.project == FABULOUSLY_OPTIMIZED),
        "{page:?}"
    );

    let project = modrinth.project(FABULOUSLY_OPTIMIZED).await.unwrap();
    assert_eq!(project.title, "Fabulously Optimized");

    let versions = modrinth.versions(FABULOUSLY_OPTIMIZED).await.unwrap();
    assert!(!versions.is_empty());
    let dates: Vec<&str> = versions
        .iter()
        .filter_map(|version| version.published.as_deref())
        .collect();
    assert!(
        dates.windows(2).all(|pair| pair[0] >= pair[1]),
        "newest first"
    );
    let pack = versions
        .iter()
        .find_map(|version| version.pack_file.as_ref())
        .expect("a version with a pack file");
    assert_eq!(pack.checksum.algorithm(), HashAlgorithm::Sha512);
    assert!(pack.url.starts_with("https://cdn.modrinth.com/"));

    let known = modrinth
        .known_files(&[KNOWN_SHA512.to_owned(), "0".repeat(128)])
        .await
        .unwrap();
    assert_eq!(known.len(), 1, "{known:?}");
    assert!(known[KNOWN_SHA512][0].starts_with("https://cdn.modrinth.com/"));
}
