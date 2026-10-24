#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;

use crate::domain::instance::GameVersion;
use crate::domain::loader::LoaderKind;
use crate::domain::mods::{DependencyKind, ModQuery, ModSource, ModTarget, ReleaseChannel};
use crate::infrastructure::modrinth::ModrinthSource;
use crate::testing::FakeHttpClient;

const BASE: &str = "https://api.test/v2";

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/modrinth/mods/",
            $name
        ))
    };
}

/// Captured 2026-10-24: Iris on Fabric/Quilt for 1.21.1, the Sodium version
/// one of its betas pins, and the Iris and Sodium projects.
const SEARCH: &[u8] = fixture!("search.json");
const IRIS_VERSIONS: &[u8] = fixture!("iris-versions.json");
const SODIUM_VERSION: &[u8] = fixture!("version.json");
const PROJECTS: &[u8] = fixture!("projects.json");
/// `POST /version_files` for the Iris 1.8.8 and Sodium jars and an unknown digest.
const VERSION_FILES: &[u8] = fixture!("version-files.json");
const IRIS_JAR: &str = "2e6ba2ffa1e1a6799288245a7e0ac68ee8df1d41b98362189df58f535cae34fa9277801e4136633467341b7dae5be0e5c698011b480b3d91b66d3dd4f7567aa6";
const SODIUM_JAR: &str = "6d4958fd3c23c0e5fdcbfcdaa3e73511bf89a0988370ab94c93e740ec2452649165b3a331351e12d392ccf924affdc042b3fc01b2e6289b7d9c7c4591c7dbeeb";

const IRIS: &str = "YL57xq9U";
const SODIUM: &str = "AANobbMI";

fn source(bodies: Vec<(String, Vec<u8>)>) -> ModrinthSource<FakeHttpClient> {
    ModrinthSource::with_base_url(FakeHttpClient::with(bodies), BASE)
}

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

fn target(loader: LoaderKind) -> ModTarget {
    ModTarget {
        game_version: GameVersion::parse("1.21.1").unwrap(),
        loader,
    }
}

fn edited(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(bytes).unwrap();
    edit(&mut value);
    serde_json::to_vec(&value).unwrap()
}

#[tokio::test]
async fn search_asks_for_client_mods_of_the_target_version_and_loaders() {
    // Quilt instances also run Fabric mods.
    let expected = url(
        "/search?query=iris%20shaders&facets=%5B%5B%22project_type%3Amod%22%5D%2C%5B%22versions%3A1.21.1%22%5D%2C%5B%22categories%3Aquilt%22%2C%22categories%3Afabric%22%5D%2C%5B%22client_side%3Arequired%22%2C%22client_side%3Aoptional%22%5D%5D&index=relevance&offset=20&limit=10",
    );
    let modrinth = source(vec![(expected.clone(), SEARCH.to_vec())]);
    let query = ModQuery {
        text: " iris shaders ".into(),
        target: target(LoaderKind::Quilt),
        page: 2,
        page_size: 10,
    };
    let page = modrinth.search(&query).await.unwrap();
    assert_eq!(modrinth.http.requests(), [expected]);
    assert_eq!(page.total, Some(23));
    assert_eq!(page.items.len(), 3);
    let iris = &page.items[0];
    assert_eq!(iris.project, IRIS);
    assert_eq!(iris.title, "Iris Shaders");
    assert!(iris.icon_url.is_some());
    assert!(iris.downloads.is_some());
}

#[tokio::test]
async fn search_on_vanilla_finds_nothing_without_asking() {
    let modrinth = source(Vec::new());
    let query = ModQuery {
        text: "iris".into(),
        target: target(LoaderKind::Vanilla),
        page: 0,
        page_size: 10,
    };
    let page = modrinth.search(&query).await.unwrap();
    assert!(page.items.is_empty());
    assert!(modrinth.http.requests().is_empty());
}

fn versions_url() -> String {
    url(&format!(
        "/project/{IRIS}/version?loaders=%5B%22quilt%22%2C%22fabric%22%5D&game_versions=%5B%221.21.1%22%5D"
    ))
}

#[tokio::test]
async fn versions_are_filtered_by_target_newest_first_with_dependencies() {
    let modrinth = source(vec![(versions_url(), IRIS_VERSIONS.to_vec())]);
    let versions = modrinth
        .versions(IRIS, &target(LoaderKind::Quilt))
        .await
        .unwrap();
    assert_eq!(modrinth.http.requests(), [versions_url()]);
    assert_eq!(versions.len(), 12);

    let newest = &versions[0];
    assert_eq!(newest.id, "bAo1Qhte");
    assert_eq!(newest.project, IRIS);
    assert_eq!(newest.channel, ReleaseChannel::Beta);
    assert_eq!(newest.version_number, "1.8.14-beta.1+1.21.1-fabric");
    assert_eq!(
        newest.published.as_deref(),
        Some("2026-06-13T15:56:42.633700Z")
    );
    let file = newest.file.as_ref().unwrap();
    assert_eq!(file.filename, "iris-fabric-1.8.14-beta.1+mc1.21.1.jar");
    assert_eq!(file.size, 2_791_343);
    assert!(file.sha1.is_some());
    // Iris needs Sodium; this beta pins one Sodium version.
    assert_eq!(newest.dependencies.len(), 1);
    let sodium = &newest.dependencies[0];
    assert_eq!(sodium.kind, DependencyKind::Required);
    assert_eq!(sodium.project.as_deref(), Some(SODIUM));
    assert_eq!(sodium.version.as_deref(), Some("s7adptIg"));

    let release = versions
        .iter()
        .find(|version| version.channel == ReleaseChannel::Release)
        .unwrap();
    assert_eq!(release.id, "zsoi0dso");
    assert_eq!(release.dependencies[0].version, None);

    let dates: Vec<_> = versions
        .iter()
        .map(|version| version.published.clone())
        .collect();
    let mut sorted = dates.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(dates, sorted);
}

#[tokio::test]
async fn versions_drop_what_does_not_fit_and_odd_dependencies_and_files() {
    let body = edited(IRIS_VERSIONS, |value| {
        let versions = value.as_array_mut().unwrap();
        // A version for another Minecraft version slipped through.
        versions[1]["game_versions"] = serde_json::json!(["1.20.1"]);
        // An unknown dependency type, and one naming nothing valid.
        versions[0]["dependencies"] = serde_json::json!([
            { "project_id": SODIUM, "dependency_type": "suggested" },
            { "project_id": SODIUM, "dependency_type": null },
            { "project_id": SODIUM },
            { "project_id": "../x", "version_id": null, "dependency_type": "required" },
            { "project_id": "P7dR8mSH", "dependency_type": "incompatible" }
        ]);
        // A jar served from a host the launcher does not download from.
        versions[0]["files"][0]["url"] = Value::from("https://evil.example/iris.jar");
    });
    let modrinth = source(vec![(versions_url(), body)]);
    let versions = modrinth
        .versions(IRIS, &target(LoaderKind::Quilt))
        .await
        .unwrap();
    assert_eq!(versions.len(), 11);
    let newest = &versions[0];
    assert_eq!(newest.file, None);
    assert_eq!(newest.dependencies.len(), 1);
    assert_eq!(newest.dependencies[0].kind, DependencyKind::Incompatible);
    assert_eq!(newest.dependencies[0].project.as_deref(), Some("P7dR8mSH"));
}

#[tokio::test]
async fn one_version_is_read_by_id() {
    let expected = url("/version/s7adptIg");
    let modrinth = source(vec![(expected.clone(), SODIUM_VERSION.to_vec())]);
    let version = modrinth.version("s7adptIg").await.unwrap();
    assert_eq!(modrinth.http.requests(), [expected]);
    assert_eq!(version.project, SODIUM);
    assert_eq!(version.loaders, [LoaderKind::Fabric]);
    assert_eq!(
        version.file.unwrap().filename,
        "sodium-fabric-0.8.12-beta.1+mc1.21.1.jar"
    );
    assert!(modrinth.version("../../x").await.is_err());
}

#[tokio::test]
async fn projects_are_read_in_one_batch_and_only_mods_are_kept() {
    let expected = url("/projects?ids=%5B%22AANobbMI%22%2C%22YL57xq9U%22%5D");
    let body = edited(PROJECTS, |value| {
        value[0]["project_type"] = Value::from("modpack");
    });
    let modrinth = source(vec![(expected.clone(), body)]);
    let projects = modrinth
        .projects(&[SODIUM.into(), "bad id!".into(), IRIS.into()])
        .await
        .unwrap();
    assert_eq!(modrinth.http.requests(), [expected]);
    assert_eq!(projects.len(), 1);
    let remaining = &projects[0];
    let unchanged = &serde_json::from_slice::<Value>(PROJECTS).unwrap()[1];
    assert_eq!(remaining.project, unchanged["id"].as_str().unwrap());
    assert_eq!(remaining.title, unchanged["title"].as_str().unwrap());
}

#[tokio::test]
async fn undated_versions_come_last_in_the_order_given() {
    let body = edited(IRIS_VERSIONS, |value| {
        let versions = value.as_array_mut().unwrap();
        versions[0]["date_published"] = Value::Null;
        versions[1]
            .as_object_mut()
            .unwrap()
            .remove("date_published");
    });
    let modrinth = source(vec![(versions_url(), body)]);
    let versions = modrinth
        .versions(IRIS, &target(LoaderKind::Quilt))
        .await
        .unwrap();
    let ids: Vec<&str> = versions.iter().map(|version| version.id.as_str()).collect();
    assert_eq!(ids[10..], ["bAo1Qhte", "zsoi0dso"]);
}

#[tokio::test]
async fn projects_are_read_in_batches_of_a_hundred() {
    let ids: Vec<String> = (0..150).map(|n| format!("mod{n:05}")).collect();
    let encoded = |batch: &[String]| {
        url(&format!(
            "/projects?ids={}",
            crate::infrastructure::modrinth::encode(&serde_json::to_string(batch).unwrap())
        ))
    };
    let first = encoded(&ids[..100]);
    let second = encoded(&ids[100..]);
    let one = edited(PROJECTS, |value| {
        value.as_array_mut().unwrap().truncate(1);
    });
    let modrinth = source(vec![(first.clone(), one.clone()), (second.clone(), one)]);
    let projects = modrinth.projects(&ids).await.unwrap();
    assert_eq!(modrinth.http.requests(), [first, second]);
    assert_eq!(projects.len(), 2);
}

#[tokio::test]
async fn identify_posts_valid_digests_once_and_maps_them_to_versions() {
    let unknown = "f".repeat(128);
    let modrinth = source(vec![(url("/version_files"), VERSION_FILES.to_vec())]);
    let identified = modrinth
        .identify(&[
            IRIS_JAR.to_uppercase(),
            SODIUM_JAR.to_owned(),
            IRIS_JAR.to_owned(),
            unknown.clone(),
            "not a digest".to_owned(),
        ])
        .await
        .unwrap();
    assert_eq!(identified.len(), 2);
    let iris = &identified[IRIS_JAR];
    assert_eq!(iris.project, IRIS);
    assert_eq!(iris.id, "zsoi0dso");
    assert_eq!(iris.version_number, "1.8.8+1.21.1-fabric");
    assert_eq!(identified[SODIUM_JAR].project, SODIUM);

    let posts = modrinth.http.posts();
    assert_eq!(posts.len(), 1);
    let body: Value = serde_json::from_slice(&posts[0].1).unwrap();
    assert_eq!(
        body,
        serde_json::json!({ "hashes": [IRIS_JAR, SODIUM_JAR, unknown], "algorithm": "sha512" })
    );
}

#[tokio::test]
async fn identify_ignores_digests_it_did_not_ask_for() {
    let modrinth = source(vec![(url("/version_files"), VERSION_FILES.to_vec())]);
    let identified = modrinth.identify(&[SODIUM_JAR.to_owned()]).await.unwrap();
    assert_eq!(identified.keys().collect::<Vec<_>>(), [SODIUM_JAR]);
    // Nothing to look up asks nothing.
    let quiet = source(Vec::new());
    assert!(quiet.identify(&[]).await.unwrap().is_empty());
    assert!(quiet.http.posts().is_empty());
}
