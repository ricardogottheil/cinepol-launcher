#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;
use crate::domain::instance::GameVersion;
use crate::domain::integrity::HashAlgorithm;
use crate::domain::loader::LoaderKind;
use crate::domain::modpack::{ModpackQuery, ModpackSource, ModpackSourceError, ModpackSourceKind};
use crate::domain::ports::{HttpClient, HttpError};
use crate::testing::FakeHttpClient;

const BASE: &str = "https://api.test/v2";

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/modrinth/",
            $name
        ))
    };
}

const SEARCH: &[u8] = fixture!("search.json");
const PROJECT: &[u8] = fixture!("project.json");
const VERSIONS: &[u8] = fixture!("versions.json");
const VERSION_FILES: &[u8] = fixture!("version_files.json");
const INDEX: &[u8] = fixture!("modrinth.index.json");

/// The first two sha512 hashes of the Fabulously Optimized 14.1.1 index,
/// the ones `version_files.json` was captured for.
const BETTER_GRASSIFY: &str = "7b70e796cea2ee57a6022108092517b6aa39d5a19b8da1de26685ae53b8fbb4717cd91ee1d232c54efa8e210ebe9b760d3dab79bf9a5fdb5ae75d97b461a1fab";
const CONFIG_MANAGER: &str = "bfa70defff2530780f0f0aa0de1845dc256d9cab244123e4847a6b24db1f812c47585ed1ddc80b2de91671aada421027b72d44e8b748e2a70ae405af54d94de0";

fn source(bodies: Vec<(String, Vec<u8>)>) -> ModrinthSource<FakeHttpClient> {
    ModrinthSource::with_base_url(FakeHttpClient::with(bodies), BASE)
}

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

fn edited(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(bytes).unwrap();
    edit(&mut value);
    serde_json::to_vec(&value).unwrap()
}

const FACETS_MODPACK: &str = "%5B%5B%22project_type%3Amodpack%22%5D%5D";

#[tokio::test]
async fn search_encodes_the_text_facets_and_paging() {
    let expected = url(
        "/search?query=fabulously%20optimized%20%26%3D%3F&facets=%5B%5B%22project_type%3Amodpack%22%5D%2C%5B%22versions%3A26.2%22%5D%2C%5B%22categories%3Afabric%22%5D%5D&index=relevance&offset=100&limit=50",
    );
    let modrinth = source(vec![(expected.clone(), SEARCH.to_vec())]);
    let query = ModpackQuery {
        text: "  fabulously optimized &=? ".into(),
        game_version: Some(GameVersion::parse("26.2").unwrap()),
        loader: Some(LoaderKind::Fabric),
        page: 2,
        page_size: 500,
    };
    modrinth.search(&query).await.unwrap();
    assert_eq!(modrinth.http.requests(), [expected]);
}

#[tokio::test]
async fn search_without_filters_only_asks_for_modpacks() {
    let expected = url(&format!(
        "/search?query=&facets={FACETS_MODPACK}&index=relevance&offset=0&limit=20"
    ));
    let modrinth = source(vec![(expected.clone(), SEARCH.to_vec())]);
    let query = ModpackQuery {
        loader: Some(LoaderKind::Vanilla),
        page_size: 20,
        ..Default::default()
    };
    modrinth.search(&query).await.unwrap();
    assert_eq!(modrinth.http.requests(), [expected]);
}

#[tokio::test]
async fn search_maps_the_hits_of_the_fixture() {
    let expected = url(&format!(
        "/search?query=&facets={FACETS_MODPACK}&index=relevance&offset=0&limit=3"
    ));
    let modrinth = source(vec![(expected, SEARCH.to_vec())]);
    let page = modrinth
        .search(&ModpackQuery {
            page_size: 3,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, Some(1530));
    let projects: Vec<&str> = page
        .items
        .iter()
        .map(|item| item.project.as_str())
        .collect();
    assert_eq!(projects, ["1KVo5zza", "ch7UHY2J", "1ocGzRHv"]);
    let first = &page.items[0];
    assert_eq!(first.source, ModpackSourceKind::Modrinth);
    assert_eq!(first.title, "Fabulously Optimized");
    assert!(first.description.starts_with("Beautiful graphics"));
    assert_eq!(
        first.icon_url.as_deref(),
        Some(
            "https://cdn.modrinth.com/data/1KVo5zza/d8152911f8fd5d7e9a8c499fe89045af81fe816e_96.webp"
        )
    );
    assert_eq!(first.downloads, Some(18_855_928));
}

#[tokio::test]
async fn project_maps_the_fixture() {
    let modrinth = source(vec![(url("/project/1KVo5zza"), PROJECT.to_vec())]);
    let project = modrinth.project("1KVo5zza").await.unwrap();
    assert_eq!(project.project, "1KVo5zza");
    assert_eq!(project.title, "Fabulously Optimized");
    assert_eq!(project.source, ModpackSourceKind::Modrinth);
    assert_eq!(project.downloads, Some(18_933_003));
    assert!(project.icon_url.is_some());
}

#[tokio::test]
async fn project_accepts_slugs() {
    let modrinth = source(vec![(
        url("/project/fabulously-optimized"),
        PROJECT.to_vec(),
    )]);
    let project = modrinth.project("fabulously-optimized").await.unwrap();
    assert_eq!(project.project, "1KVo5zza", "the stable id, not the slug");
}

#[tokio::test]
async fn invalid_project_ids_are_not_found_without_a_request() {
    let modrinth = source(vec![]);
    for id in [
        "",
        "../search",
        "a/b",
        "a b",
        "x?y=1",
        "%2e%2e",
        &"a".repeat(65),
    ] {
        assert_eq!(
            modrinth.project(id).await,
            Err(ModpackSourceError::NotFound(id.to_owned())),
            "{id}"
        );
        assert_eq!(
            modrinth.versions(id).await,
            Err(ModpackSourceError::NotFound(id.to_owned())),
            "{id}"
        );
    }
    assert!(modrinth.http.requests().is_empty());
}

#[tokio::test]
async fn projects_that_are_not_modpacks_are_not_found() {
    let modrinth = source(vec![(
        url("/project/AANobbMI"),
        edited(PROJECT, |project| project["project_type"] = json!("mod")),
    )]);
    assert_eq!(
        modrinth.project("AANobbMI").await,
        Err(ModpackSourceError::NotFound("AANobbMI".into()))
    );
}

#[tokio::test]
async fn versions_map_the_fixture_newest_first() {
    let modrinth = source(vec![(url("/project/1KVo5zza/version"), VERSIONS.to_vec())]);
    let versions = modrinth.versions("1KVo5zza").await.unwrap();
    let ids: Vec<&str> = versions.iter().map(|version| version.id.as_str()).collect();
    assert_eq!(ids, ["D35n5vLL", "uQxD0ENB", "glnu8pns"]);

    let stable = &versions[1];
    assert_eq!(stable.version_number, "14.1.1");
    assert_eq!(stable.game_versions, [GameVersion::parse("26.2").unwrap()]);
    assert_eq!(stable.loaders, [LoaderKind::Fabric]);
    assert_eq!(
        stable.published.as_deref(),
        Some("2026-10-11T10:32:15.321598Z")
    );
    let pack = stable.pack_file.as_ref().unwrap();
    assert_eq!(
        pack.url,
        "https://cdn.modrinth.com/data/1KVo5zza/versions/uQxD0ENB/Fabulously.Optimized-v14.1.1.mrpack"
    );
    assert_eq!(pack.filename, "Fabulously.Optimized-v14.1.1.mrpack");
    assert_eq!(pack.checksum.algorithm(), HashAlgorithm::Sha512);
    assert_eq!(
        pack.checksum.value(),
        "97bcd5c52362fd4d5add5a476eeb2565f46f685785cc062c51b2bb58f30a91c2dfd44ceea1546f3aed46785ce421ef4a33dd36f99c783b0f3af9ddbe40d1624e"
    );
    assert!(pack.size > 0);
    assert!(
        versions.iter().all(|version| version.pack_file.is_some()),
        "the cosign bundle is never picked"
    );
}

fn version(id: &str, published: Option<&str>, files: Value) -> Value {
    json!({
        "id": id,
        "name": format!("Version {id}"),
        "version_number": id,
        "game_versions": ["26.2", "not a version!"],
        "loaders": ["fabric", "minecraft", "datapack"],
        "date_published": published,
        "files": files,
    })
}

fn file(filename: &str, primary: bool, sha512: &str) -> Value {
    json!({
        "url": format!("https://cdn.modrinth.com/data/p/versions/v/{filename}"),
        "filename": filename,
        "primary": primary,
        "size": 7,
        "hashes": { "sha1": "0f4a890d07402280686a518579fa9fa02309e315", "sha512": sha512 },
    })
}

#[tokio::test]
async fn versions_sort_by_publication_and_skip_unknown_values() {
    let hash = "a".repeat(128);
    let list = json!([
        version(
            "old",
            Some("2025-01-01T00:00:00Z"),
            json!([file("old.mrpack", true, &hash)])
        ),
        version(
            "undated",
            None,
            json!([file("undated.mrpack", true, &hash)])
        ),
        version(
            "new",
            Some("2026-01-01T00:00:00.5Z"),
            json!([file("new.mrpack", true, &hash)])
        ),
        version(
            "same-second",
            Some("2026-01-01T00:00:00Z"),
            json!([file("s.mrpack", true, &hash)])
        ),
    ]);
    let modrinth = source(vec![(
        url("/project/abc/version"),
        serde_json::to_vec(&list).unwrap(),
    )]);
    let versions = modrinth.versions("abc").await.unwrap();
    let ids: Vec<&str> = versions.iter().map(|version| version.id.as_str()).collect();
    assert_eq!(ids, ["new", "same-second", "old", "undated"]);
    assert_eq!(
        versions[0].game_versions,
        [GameVersion::parse("26.2").unwrap()]
    );
    assert_eq!(versions[0].loaders, [LoaderKind::Fabric]);
}

async fn pack_file_of(files: Value) -> Option<crate::domain::modpack::ModpackFile> {
    let list = json!([version("v", Some("2026-01-01T00:00:00Z"), files)]);
    let modrinth = source(vec![(
        url("/project/abc/version"),
        serde_json::to_vec(&list).unwrap(),
    )]);
    modrinth.versions("abc").await.unwrap().remove(0).pack_file
}

#[tokio::test]
async fn the_pack_file_is_the_primary_mrpack_or_the_first_one() {
    let hash = "b".repeat(128);
    let primary = pack_file_of(json!([
        file("first.mrpack", false, &hash),
        file("main.mrpack", true, &hash),
    ]))
    .await
    .unwrap();
    assert_eq!(primary.filename, "main.mrpack");

    let fallback = pack_file_of(json!([
        file("readme.zip", true, &hash),
        file("first.mrpack", false, &hash),
        file("second.mrpack", false, &hash),
    ]))
    .await
    .unwrap();
    assert_eq!(fallback.filename, "first.mrpack");
    assert_eq!(fallback.checksum.value(), hash);
    assert_eq!(fallback.size, 7);
}

#[tokio::test]
async fn a_pack_file_that_cannot_be_verified_or_is_off_the_allowed_hosts_is_dropped() {
    assert_eq!(
        pack_file_of(json!([file("a.zip", true, &"c".repeat(128))])).await,
        None
    );
    assert_eq!(
        pack_file_of(json!([file("a.mrpack", true, "abc")])).await,
        None
    );
    let mut unhashed = file("a.mrpack", true, &"c".repeat(128));
    unhashed["hashes"].as_object_mut().unwrap().remove("sha512");
    assert_eq!(pack_file_of(json!([unhashed])).await, None);
    let mut foreign = file("a.mrpack", true, &"c".repeat(128));
    foreign["url"] = json!("https://evil.example/a.mrpack");
    assert_eq!(pack_file_of(json!([foreign])).await, None);
    let mut traversal = file("a.mrpack", true, &"c".repeat(128));
    traversal["filename"] = json!("../../a.mrpack");
    assert_eq!(pack_file_of(json!([traversal])).await, None);
}

#[tokio::test]
async fn known_files_posts_the_hashes_and_keeps_only_the_matching_file() {
    let unknown = "d".repeat(128);
    let modrinth = source(vec![(url("/version_files"), VERSION_FILES.to_vec())]);
    let known = modrinth
        .known_files(&[
            BETTER_GRASSIFY.to_owned(),
            CONFIG_MANAGER.to_owned(),
            unknown.clone(),
        ])
        .await
        .unwrap();
    assert_eq!(
        known,
        HashMap::from([
            (
                BETTER_GRASSIFY.to_owned(),
                vec![
                    "https://cdn.modrinth.com/data/m5T5xmUy/versions/r4yqxYQl/BetterGrassify-1.8.7%2Bfabric.26.2.jar"
                        .to_owned()
                ]
            ),
            (
                CONFIG_MANAGER.to_owned(),
                vec![
                    "https://cdn.modrinth.com/data/jlNms3Jp/versions/W4C08wBC/ConfigManager-fabric-26.1_26.2-1.1.3.jar"
                        .to_owned()
                ]
            ),
        ])
    );
    let posts = modrinth.http.posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].0, url("/version_files"));
    let body: Value = serde_json::from_slice(&posts[0].1).unwrap();
    assert_eq!(
        body,
        json!({ "hashes": [BETTER_GRASSIFY, CONFIG_MANAGER, unknown], "algorithm": "sha512" })
    );
}

#[tokio::test]
async fn the_fixture_hashes_come_from_the_pack_index() {
    let index: Value = serde_json::from_slice(INDEX).unwrap();
    let hashes: Vec<&str> = index["files"]
        .as_array()
        .unwrap()
        .iter()
        .take(2)
        .map(|file| file["hashes"]["sha512"].as_str().unwrap())
        .collect();
    assert_eq!(hashes, [BETTER_GRASSIFY, CONFIG_MANAGER]);
}

#[tokio::test]
async fn known_files_drops_urls_off_the_allowed_hosts_and_answers_not_asked_for() {
    let foreign = edited(VERSION_FILES, |answer| {
        answer[CONFIG_MANAGER]["files"][0]["url"] = json!("https://evil.example/a.jar");
    });
    let modrinth = source(vec![(url("/version_files"), foreign)]);
    let known = modrinth
        .known_files(&[BETTER_GRASSIFY.to_owned(), CONFIG_MANAGER.to_owned()])
        .await
        .unwrap();
    assert_eq!(known.keys().collect::<Vec<_>>(), [BETTER_GRASSIFY]);

    let modrinth = source(vec![(url("/version_files"), VERSION_FILES.to_vec())]);
    let known = modrinth
        .known_files(&[BETTER_GRASSIFY.to_owned()])
        .await
        .unwrap();
    assert_eq!(known.keys().collect::<Vec<_>>(), [BETTER_GRASSIFY]);
}

#[tokio::test]
async fn known_files_lowercases_and_skips_invalid_hashes() {
    let modrinth = source(vec![(url("/version_files"), VERSION_FILES.to_vec())]);
    let known = modrinth
        .known_files(&[
            BETTER_GRASSIFY.to_ascii_uppercase(),
            "not-a-hash".into(),
            CONFIG_MANAGER.to_owned(),
            BETTER_GRASSIFY.to_owned(),
        ])
        .await
        .unwrap();
    assert!(known.contains_key(BETTER_GRASSIFY));
    let body: Value = serde_json::from_slice(&modrinth.http.posts()[0].1).unwrap();
    assert_eq!(body["hashes"], json!([BETTER_GRASSIFY, CONFIG_MANAGER]));
}

#[tokio::test]
async fn known_files_without_valid_hashes_makes_no_request() {
    let modrinth = source(vec![]);
    assert!(modrinth.known_files(&[]).await.unwrap().is_empty());
    assert!(
        modrinth
            .known_files(&["xyz".into()])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(modrinth.http.requests().is_empty());
}

#[tokio::test]
async fn known_files_sends_batches_of_five_hundred() {
    let modrinth = source(vec![(url("/version_files"), b"{}".to_vec())]);
    let hashes: Vec<String> = (0..1001).map(|n| format!("{n:0128x}")).collect();
    modrinth.known_files(&hashes).await.unwrap();
    let sizes: Vec<usize> = modrinth
        .http
        .posts()
        .iter()
        .map(|(_, body)| {
            let body: Value = serde_json::from_slice(body).unwrap();
            body["hashes"].as_array().unwrap().len()
        })
        .collect();
    assert_eq!(sizes, [500, 500, 1]);
}

/// Answers every request with the same error.
struct Failing(HttpError);

impl HttpClient for Failing {
    async fn get_bytes(&self, _url: &str) -> Result<Vec<u8>, HttpError> {
        Err(self.0.clone())
    }

    async fn post_json(&self, _url: &str, _body: Vec<u8>) -> Result<Vec<u8>, HttpError> {
        Err(self.0.clone())
    }
}

#[tokio::test]
async fn http_errors_map_to_source_errors() {
    let failing = |error: HttpError| ModrinthSource::with_base_url(Failing(error), BASE);
    assert_eq!(
        failing(HttpError::NotFound("x".into()))
            .project("gone")
            .await,
        Err(ModpackSourceError::NotFound("gone".into()))
    );
    assert_eq!(
        failing(HttpError::RateLimited {
            url: "x".into(),
            retry_after_secs: 30
        })
        .versions("abc")
        .await,
        Err(ModpackSourceError::RateLimited {
            retry_after_secs: 30
        })
    );
    let network = failing(HttpError::Network {
        url: "x".into(),
        reason: "offline".into(),
    })
    .known_files(&[BETTER_GRASSIFY.to_owned()])
    .await;
    assert!(
        matches!(network, Err(ModpackSourceError::Network(_))),
        "{network:?}"
    );
    let status = failing(HttpError::Status {
        url: "x".into(),
        status: 400,
    })
    .search(&ModpackQuery::default())
    .await;
    assert!(
        matches!(status, Err(ModpackSourceError::Network(_))),
        "{status:?}"
    );
}

#[tokio::test]
async fn malformed_answers_are_invalid_responses() {
    let modrinth = source(vec![
        (url("/project/abc"), b"not json".to_vec()),
        (url("/project/abc/version"), b"{}".to_vec()),
        (url("/version_files"), b"[]".to_vec()),
    ]);
    assert!(matches!(
        modrinth.project("abc").await,
        Err(ModpackSourceError::InvalidResponse(_))
    ));
    assert!(matches!(
        modrinth.versions("abc").await,
        Err(ModpackSourceError::InvalidResponse(_))
    ));
    assert!(matches!(
        modrinth.known_files(&[BETTER_GRASSIFY.to_owned()]).await,
        Err(ModpackSourceError::InvalidResponse(_))
    ));
}

#[test]
fn reports_its_kind_and_the_public_api_by_default() {
    let modrinth = ModrinthSource::new(FakeHttpClient::default());
    assert_eq!(modrinth.kind(), ModpackSourceKind::Modrinth);
    assert_eq!(modrinth.base_url, MODRINTH_API);
}
